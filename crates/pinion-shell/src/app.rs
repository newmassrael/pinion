//! R51.92.1 §5.40 — surface (winit / wgpu / `accesskit_winit`) module
//! split from `lib.rs`.
//!
//! Houses the framework-side application surface ([`AppShell`]) and
//! its `winit::application::ApplicationHandler` implementation, plus
//! the two surface-only helpers (`named_key_str` for the winit↔W3C
//! `KeyboardEvent.key` bridge and `spawn_stdin_rpc_reader` for the
//! background JSON-RPC reader thread) and the public [`run`]
//! entry-point every visual binary's `fn main()` collapses to.
//!
//! R51.92 (substrate.rs) extracted [`crate::ShellCore`] so the
//! dispatch substrate is module-private (14 fields + the
//! [`crate::AccessEmitDecision`] body genuinely private to
//! `substrate.rs`). R51.92.1 completes the textbook 3-module split:
//! every `AppShell` field — including the `core: ShellCore<V>`
//! borrow — is private to `app.rs`, so the surface boundary is now
//! enforced at the module level as well. Any future addition to
//! `lib.rs` cannot accidentally reach across the boundary to touch
//! either the substrate's fields or `AppShell`'s winit surface
//! state.
//!
//! See `claim-accuracy-self-audit` and `substrate-incompleteness-signal`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use pinion_core::reactive::{Effect, Signal};

use pinion_a11y::AccessTreeBuilder;
use pinion_core::event::WheelDelta;
use pinion_core::scene::BoxNode;
use pinion_runtime::{CommandExecutor, HandlerRegistry, PointerId, image_cache, paint_adapter};
use vello::Scene as VelloScene;
use vello::kurbo::Affine;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{ResizeDirection, Window, WindowId, WindowLevel};

use crate::executor::build_executor_and_sink;
use crate::substrate::ShellCore;
use crate::{
    AppEvent, RenderState, SizeStrategy, VelloContext, VelloRenderer, WidgetRenderer, WidgetView,
    WindowSpec,
};

/// R670.B §5.16 §5.41 — per-window state cluster.
///
/// Lifts the 5 single-window `AppShell` fields (`render` +
/// `vello_scene` + `accesskit` + IME state + `pending_intrinsic_resize`)
/// into one struct so [`AppShell`] can hold
/// `HashMap<WindowId, WindowSlot<R>>` and stay disjoint across
/// multi-window dispatch. R670.A landed the
/// `WidgetView::windows() -> Vec<WindowSpec>` trait foundation; this
/// struct is the runtime side that the foundation enables — each
/// `WindowSpec` in the binding's list owns one `WindowSlot` once
/// [`AppShell::resumed`] creates the winit `Window` + GPU renderer +
/// AccessKit adapter for that spec.
///
/// The pre-R670.B single-window `AppShell` carried these fields
/// directly on the struct. R670.B preserves bit-identical
/// single-window lifecycle by anchoring the canonical primary spec
/// (`WindowSpec::main(...)` from R670.A default impl) as the only
/// window the 15+ existing bindings ever create — the `HashMap`
/// holds exactly one entry for single-window bindings and the
/// per-window dispatch code paths are no-op-per-spec for the missing
/// secondaries.
/// (R1121 §5.16 §5.39) A client-side window-chrome control a left-press
/// resolved to (a borderless window's title-bar buttons / drag grip). Mapped
/// from the [`pinion_overlay`] chrome control tag in [`AppShell::try_chrome_press`].
#[derive(Clone, Copy)]
enum ChromeAction {
    /// Close the window (routes to `event_loop.exit()`, the `CloseRequested` path).
    Close,
    /// `Window::set_minimized(true)`.
    Minimize,
    /// `Window::set_maximized(toggle)`.
    Maximize,
    /// `Window::drag_window()` — OS-driven interactive move from the grip.
    Move,
    /// `Window::drag_resize_window(dir)` — OS-driven interactive resize from a
    /// client-side resize edge / corner (R1122). A borderless window has no OS
    /// frame, so the chrome supplies the resize border.
    Resize(ResizeDirection),
}

/// (R1121 §5.16 §5.39) Map a hit-test tag to the window-chrome control it
/// names, or `None` when the tag is not a chrome control. Pure (uses only the
/// `ResizeDirection` enum value, no live winit `Window` / `self`) so the
/// tag→action contract is unit-tested without a live window.
fn chrome_action_for_tag(tag: &str) -> Option<ChromeAction> {
    match tag {
        pinion_overlay::WINDOW_CHROME_CLOSE_TAG => Some(ChromeAction::Close),
        pinion_overlay::WINDOW_CHROME_MINIMIZE_TAG => Some(ChromeAction::Minimize),
        pinion_overlay::WINDOW_CHROME_MAXIMIZE_TAG => Some(ChromeAction::Maximize),
        pinion_overlay::WINDOW_CHROME_GRIP_TAG => Some(ChromeAction::Move),
        // R1122 — the eight resize edges / corners.
        pinion_overlay::WINDOW_RESIZE_NORTH_TAG => {
            Some(ChromeAction::Resize(ResizeDirection::North))
        }
        pinion_overlay::WINDOW_RESIZE_SOUTH_TAG => {
            Some(ChromeAction::Resize(ResizeDirection::South))
        }
        pinion_overlay::WINDOW_RESIZE_WEST_TAG => Some(ChromeAction::Resize(ResizeDirection::West)),
        pinion_overlay::WINDOW_RESIZE_EAST_TAG => Some(ChromeAction::Resize(ResizeDirection::East)),
        pinion_overlay::WINDOW_RESIZE_NORTH_WEST_TAG => {
            Some(ChromeAction::Resize(ResizeDirection::NorthWest))
        }
        pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG => {
            Some(ChromeAction::Resize(ResizeDirection::NorthEast))
        }
        pinion_overlay::WINDOW_RESIZE_SOUTH_WEST_TAG => {
            Some(ChromeAction::Resize(ResizeDirection::SouthWest))
        }
        pinion_overlay::WINDOW_RESIZE_SOUTH_EAST_TAG => {
            Some(ChromeAction::Resize(ResizeDirection::SouthEast))
        }
        _ => None,
    }
}

struct WindowSlot<R: VelloRenderer> {
    /// R46.5 §5.16 suspend / resume lifecycle (R46.3.4 pattern).
    /// Lifted from the single-window `AppShell` field of the same name.
    render: RenderState<R>,
    /// Reusable Vello scene buffer — reset at the start of each
    /// frame rather than reallocated. Vello's Scene API expects this
    /// pattern (Linebender Vello examples / Xilem). Per-window because
    /// multi-window paint would otherwise serialise the same buffer
    /// across windows (race on `reset` between secondary + primary
    /// paint cycles).
    vello_scene: VelloScene,
    /// R51.62 §5.40 — per-window `accesskit_winit::Adapter`. `None`
    /// while the window is `Suspended`; populated once the winit
    /// `Window` exists. AccessKit canonical: 1 adapter = 1 window
    /// (multi-window = multi-adapter, NOT auto-merged), mirroring
    /// macOS `NSAccessibility` / GTK Atk semantics.
    accesskit: Option<accesskit_winit::Adapter>,
    /// R56.2.a §5.13 §5.38 — per-window IME composition state.
    /// Per-window because the IME session belongs to the focused
    /// window (Wayland `text-input-v3` / X11 XIM / macOS
    /// `NSTextInputContext` all scope by window); a multi-window
    /// binding's main + inspector windows could each carry their
    /// own composition session if both contained text-input widgets.
    ime_was_composing: bool,
    /// R56.2.c §5.13 §5.38 — per-window last
    /// `Window::set_ime_cursor_area` rect. Per-window so dedup against
    /// winit boundary calls works correctly when different windows
    /// publish different caret positions.
    last_ime_cursor_area: Option<(f32, f32, f32, f32)>,
    /// R668 §5.16 — per-window pending
    /// [`SizeStrategy::IntrinsicAfterFirstPaint`] resize. Per-window
    /// because the `IntrinsicAfterFirstPaint` contract is one-shot
    /// per-window-lifetime (not per-binding); mixed-strategy bindings
    /// (main: `Fixed` + inspector: `IntrinsicAfterFirstPaint`) need
    /// independent resize queues.
    pending_intrinsic_resize: Option<((u32, u32), (u32, u32))>,
    /// R670.B §5.16 — spec id (the canonical `&'static str` AI
    /// clients address the window by). Cached here so per-window
    /// dispatch code (RPC scope resolution, paint scene producer,
    /// future `view_for_window` plumbing) can resolve the spec id from
    /// the winit `WindowId` without re-walking
    /// [`AppShell::spec_id_to_window_id`]. Canonical primary spec is
    /// `"main"`; secondary specs pick their own non-conflicting
    /// names.
    ///
    /// Read by [`AppShell::render_window`] (per-window redraw drain
    /// keyed on `spec_id`) + [`AppShell::dispatch_rpc`] window-scope
    /// resolution.
    ///
    /// R683 §5.16 — `Cow<'static, str>` so dock + tear-off can mint
    /// runtime ids (`Cow::Owned(format!("torn-panel-{n}"))`)
    /// alongside the canonical static literals
    /// (`Cow::Borrowed("main")` / `Cow::Borrowed("inspector")`). All
    /// downstream `spec_id: &str` parameter sites stay unchanged —
    /// Cow's `Deref<Target = str>` covers the read API.
    spec_id: Cow<'static, str>,
    /// R682 §5.16 atomic 1 — per-window paint-fragment cache. Lives
    /// per `WindowSlot` because the cached `vello::Scene` fragments
    /// reference the same backend coordinate space as
    /// [`Self::vello_scene`]; sharing a cache across windows would
    /// require encoding fragments with explicit transform metadata
    /// for cross-window replay, which is out of scope for the
    /// 4-axis paint-pipeline rewrite series.
    ///
    /// The cache is consulted by [`paint_adapter::to_vello_cached`]
    /// (R682 atomic 1) at every cacheable [`Scene::Container`]
    /// boundary the encoder reaches with [`Affine::IDENTITY`]
    /// accumulated transform. A cache hit appends the previously
    /// encoded fragment via [`vello::Scene::append`] without
    /// re-walking the subtree; a miss encodes the subtree fresh,
    /// installs the fragment, and replays from the install slot.
    ///
    /// Mark-and-sweep eviction (handled inside
    /// [`FragmentCache::end_paint`]) keeps the cache bounded to the
    /// set of cacheable Containers actually painted in the most
    /// recent frame — no fixed-cap LRU; no manual reset between
    /// frames.
    fragment_cache: paint_adapter::FragmentCache,
    /// R740 §5.16 — per-window decoded-image cache. Sits beside the
    /// fragment cache (both are vello render state) and feeds
    /// [`paint_adapter::to_vello_cached`] so `Scene::Image` sources are
    /// decoded once and reused every frame. Per-window for now (a
    /// multi-window image consumer that wants one shared decode is an
    /// additive app-level move, [[abstraction-needs-second-consumer]]).
    image_cache: image_cache::ImageCache,
    /// R1027 §5.16 — the window's current winit `scale_factor` (device
    /// pixels per logical pixel). Seeded from `Window::scale_factor()` at
    /// slot construction and refreshed on
    /// `WindowEvent::ScaleFactorChanged`. The paint scene is laid out in
    /// logical pixels; this factor is applied only at the GPU raster
    /// boundary (the scaled vello append in `render_window`), the
    /// pointer-input boundary (`CursorMoved` / `Touch` physical ->
    /// logical), and the AccessKit root transform. `1.0` on a non-`HiDPI`
    /// display, which keeps the byte-identical pre-R1027 render path.
    scale_factor: f64,
    /// R1027 §5.16 — reusable scratch scene for the `HiDPI` scaled append.
    /// When `scale_factor` is non-identity, `render_window` appends the
    /// logical `vello_scene` into this buffer under an
    /// `Affine::scale(scale_factor)` so the logical scene rasterizes at
    /// device resolution; the renderer then submits this scene. At `1.0`
    /// the renderer submits `vello_scene` directly and this buffer stays
    /// untouched (no extra append, so non-`HiDPI` performance is
    /// unchanged). Per-window for the same reason `vello_scene` is (no
    /// cross-window buffer race on `reset`).
    scaled_scene: VelloScene,
    /// R1060 §5.16 §5.12 — one-shot live-surface capture request, whose
    /// set+clear lifecycle [`AppShell::capture_window_screenshot`] OWNS
    /// (R1062): it sets the flag, drives one [`AppShell::render_window`]
    /// pass, then clears it unconditionally. `render_window` only READS it
    /// and, when set, submits the frame through
    /// [`VelloRenderer::capture_rgba8`] (which reads back the presented
    /// swapchain texture) instead of the normal `render` present. The
    /// unconditional caller-side clear means an early-return `render_window`
    /// path cannot leave it stale, so it is `false` on every
    /// event-loop-driven paint and the 60-144fps hot path is byte-unchanged.
    pending_capture: bool,
    /// R1060 §5.16 §5.12 — the most recent capture result, written by
    /// `render_window` when `pending_capture` was set and drained by
    /// `capture_window_screenshot` immediately after. `None` between
    /// captures.
    last_capture: Option<crate::vello_capture::CapturedFrame>,
    /// R1088 §5.16 §5.41 §2 #7 PR-31 — the last outer position the SHELL
    /// commanded for this window via `Window::set_outer_position` (the
    /// reconcile move pass or the `Suspended`-resume re-apply), in logical
    /// px, that has not yet been observed echoing back as a
    /// `WindowEvent::Moved`. [`AppShell::note_window_moved`] suppresses a
    /// `Moved` equal to it (our own command echoing) and writes back only
    /// a DIVERGENT one (a user / WM title-bar drag), so a shell-driven
    /// move does not loop and a user move converges the declared
    /// `windows_signal` on the actual position. `None` once the echo is
    /// consumed, or when the shell has commanded no position (every
    /// WM-placed window — the typical `"main"`).
    last_commanded_position: Option<(i32, i32)>,
}

impl<R: VelloRenderer> WindowSlot<R> {
    /// R682 §5.16 — collapse the per-slot field defaults that the
    /// `resume_spec` code path repeats at both the suspended-init and
    /// active-init sites. Only `render`, `accesskit`, `spec_id`,
    /// `pending_intrinsic_resize`, and `scale_factor` (R1027) vary
    /// between the two sites; every other field starts at its canonical
    /// empty-state value.
    fn build(
        render: RenderState<R>,
        accesskit: Option<accesskit_winit::Adapter>,
        spec_id: Cow<'static, str>,
        pending_intrinsic_resize: Option<((u32, u32), (u32, u32))>,
        scale_factor: f64,
    ) -> Self {
        Self {
            render,
            vello_scene: VelloScene::new(),
            accesskit,
            ime_was_composing: false,
            last_ime_cursor_area: None,
            pending_intrinsic_resize,
            spec_id,
            fragment_cache: paint_adapter::FragmentCache::new(),
            image_cache: image_cache::ImageCache::new(),
            scale_factor,
            scaled_scene: VelloScene::new(),
            pending_capture: false,
            last_capture: None,
            last_commanded_position: None,
        }
    }
}

/// (R1147 §5.51 §5.16) The shell-private cross-desktop drag preview window — the
/// Qt-ADS `CFloatingDragPreview` model. A small, opaque, borderless,
/// always-on-top window that *is* the drag chip; it follows the desktop cursor
/// during a dock drag so the chip can escape the source window (the R1113
/// in-window overlay is clipped to its surface, the gap the user found after
/// R1146).
///
/// Created **once and reused** (hidden via `set_visible(false)` between drags —
/// no per-gesture window creation, so no R1144-class surface-churn freeze),
/// **moved by a direct `set_outer_position`** from the DESKTOP cursor (never the
/// reactive `Signal → reconcile_windows` path, and the cursor — not the window's
/// own moved position — is read, so the R1119 feedback oscillation is
/// structurally impossible), and **repainted only when the dragged label
/// changes** (render-once, move-many: zero per-move GPU work).
///
/// Deliberately ABSENT from [`AppShell::windows`] / `spec_id_to_window_id` / the
/// substrate window registry / `windows_signal`, so it never appears in
/// `scene/windows`: a transient shell affordance like the R1113 overlay + focus
/// ring, NOT declared scene-as-data (§2 #7). The during-drag chip stays
/// AI-introspectable through the R1113 in-window overlay in `scene/snapshot`,
/// which this window mirrors visually for the human.
struct DragPreviewWindow<R: VelloRenderer> {
    /// The OS window — the chip itself (opaque, borderless, always-on-top).
    window: Arc<Window>,
    /// Cached winit id so the `RedrawRequested` / `Resized` arms route a preview
    /// frame here without a `windows` lookup (the preview is not in that map).
    window_id: WindowId,
    /// Per-preview Vello renderer (its own GPU surface). Tiny fixed scene, so no
    /// fragment / image cache, no AccessKit, no IME — unlike a [`WindowSlot`].
    renderer: Box<R>,
    /// Reusable encode buffer (reset per repaint, like `WindowSlot::vello_scene`).
    vello_scene: VelloScene,
    /// OS DPI scale for the preview's monitor (seeded at create; the chip is
    /// laid out logical and rastered at device resolution like every window).
    scale_factor: f64,
    /// Whether the window is currently mapped (`set_visible(true)`). Shown on a
    /// preview-eligible drag, hidden on release.
    visible: bool,
    /// The label the chip is currently painted with (`None` until first paint).
    /// The shell repaints + resizes-to-fit only when the active drag's label
    /// differs, so a steady cursor move is a pure `set_outer_position`.
    painted_label: Option<String>,
}

/// The framework-side shell. Generic over a widget binding
/// [`WidgetView`]; concrete examples instantiate via `run::<V>()`.
///
/// R51.76 §5.40 — every piece of testable dispatch state lives in
/// [`ShellCore`]; this struct only owns the winit / wgpu / AccessKit
/// surface so headless tests can target [`ShellCore`] directly.
///
/// R51.92.1 §5.40 — moved from `lib.rs` to its own module so every
/// field below (including the `core: ShellCore<V>` substrate borrow
/// added in R51.76) is genuinely private to `app.rs`. The lib.rs
/// entry point reaches the shell only through [`run`] / the
/// re-exported `AppShell::new` constructor; private state is now
/// module-bound rather than file-bound.
pub struct AppShell<V: WidgetView> {
    /// R51.76 §5.40 — extracted dispatch substrate (scene, cached
    /// state, focus, intents, previews, revision, router, modifiers,
    /// text cache, last paint snapshot, AT caches, redraw flag).
    ///
    /// R51.83 §5.40 — private. All surface-side access happens
    /// through the substrate's typed methods + accessors so the
    /// boundary stays one-way.
    ///
    /// R51.92.1 §5.40 — module-private. Even `lib.rs` (the entry
    /// crate root) cannot touch this field directly.
    ///
    /// R670.B §5.16 — single `ShellCore` + multi-window (Approach A):
    /// the binding's state lives here once; every window's view fn
    /// reads the same `cached_state`. Multi-binding (different
    /// `ShellCore` per window) is R750+ widget catalog territory
    /// (Approach B).
    core: ShellCore<V>,
    /// R670.B §5.16 §5.41 — per-window state cluster, keyed by winit
    /// `WindowId`. R670.A landed `WidgetView::windows() -> Vec<WindowSpec>`
    /// with default `vec![WindowSpec::main(...)]`; R670.B walks the
    /// list in `resumed()` + creates one `WindowSlot` per spec.
    ///
    /// Pre-R670.B this struct carried 5 single-window fields
    /// directly (`render` + `vello_scene` + `accesskit` + IME +
    /// intrinsic resize). The cluster lift preserves bit-identical
    /// single-window behaviour by anchoring the canonical primary
    /// spec (`WindowSpec::main`) as the only window the 15+ existing
    /// bindings ever create — the hashmap holds exactly one entry
    /// for them.
    windows: HashMap<WindowId, WindowSlot<V::Renderer>>,
    /// R670.B §5.16 — spec-id → `WindowId` reverse lookup for RPC
    /// `{window: "<id>"}` scope resolution (R670.B atomic 1). AI
    /// clients address windows by the `&'static str` id declared in
    /// `WindowSpec::new(id, ..)`; the dispatcher resolves the id to a
    /// winit `WindowId` here before looking up the per-window slot
    /// in [`Self::windows`]. Default single-window bindings carry
    /// exactly `"main" → primary_id`.
    spec_id_to_window_id: HashMap<Cow<'static, str>, WindowId>,
    /// R670.B §5.16 — primary window's [`WindowId`]. The first spec
    /// in `V::windows()` (canonically `WindowSpec::main(..)`); RPC
    /// frames that omit `{window: "..."}` default-scope to this id.
    /// `None` until `resumed()` creates the first window; populated
    /// once per binding lifetime + never cleared by `suspended`
    /// (which keeps the windows cached for the next `resumed`).
    primary_window_id: Option<WindowId>,
    /// R51.62 §5.40 — winit [`EventLoopProxy`] cached so the shell
    /// can construct the per-window `accesskit_winit::Adapter` on
    /// `resumed` (the constructor requires both the active event
    /// loop and a proxy that produces `Adapter`-routed user events).
    proxy: EventLoopProxy<AppEvent>,
    /// R683 §5.16 §5.41 — runtime window-list reactive lift.
    ///
    /// Populated on the first [`Self::resumed`] when the binding's
    /// [`WidgetView::windows_signal`] returns `Some(..)`. Kept here so
    /// [`Self::reconcile_windows`] (the
    /// [`AppEvent::WindowsDirty`] handler) can re-read the latest
    /// `Signal<Vec<WindowSpec>>` snapshot without going back through
    /// the trait (which would re-evaluate the binding's
    /// `Owner::cache` factory each call — the trait impl pattern
    /// memoises but the shell-side cache makes the contract
    /// explicit).
    ///
    /// `None` for every pre-R683 single + multi-window binding —
    /// they inherit the default `fn windows_signal() -> None` and the
    /// reconcile Effect is never installed.
    windows_signal: Option<Rc<Signal<Vec<WindowSpec>>>>,
    /// R683 §5.16 §5.41 — lifetime anchor for the reconcile Effect.
    ///
    /// The Effect is registered against `self.core.root_owner()` so
    /// its cleanup queue holds a strong reference for the binding's
    /// lifetime — but the AppShell-side `Option<Effect>` keeps the
    /// handle live + accessible for diagnostics (test asserts the
    /// Effect was actually installed). `None` when
    /// `WidgetView::windows_signal()` returned `None` (no opt-in,
    /// no Effect, no reactive subscription).
    ///
    /// The handle is read in [`Self::resumed`] (the
    /// `self.reconcile_effect.is_none()` install gate) and never
    /// otherwise — its real job is to extend the [`Effect`]'s
    /// lifetime past the install scope so the Effect closure stays
    /// alive across event-loop iterations. The Owner cleanup queue
    /// also holds a `Weak<EffectInner>` so the cleanup-on-shutdown
    /// path is correct independent of this handle.
    reconcile_effect: Option<Effect>,
    /// R683 §5.16 §5.41 — last spec list snapshot the reconcile
    /// Effect observed, used to compute the add/drop diff.
    ///
    /// Initialised to `V::windows_signal().get()` on the first
    /// `resumed()` when the Effect is installed; updated to the
    /// freshly-emitted `Signal::get()` snapshot at the end of each
    /// `reconcile_windows` call. Wrapped in `Rc<RefCell<..>>`
    /// because the Effect closure cannot capture `&mut self` (it
    /// lives past the `AppShell` constructor scope); the `AppShell`
    /// + the closure share the same `Rc<RefCell<..>>` handle.
    ///
    /// An empty `Vec` sentinel for the pre-install state — when
    /// [`AppEvent::WindowsDirty`] fires before the install completes
    /// (a degenerate corner case) the diff fires "no adds, no drops"
    /// against the empty baseline.
    last_known_specs: Rc<RefCell<Vec<WindowSpec>>>,
    /// R1147 §5.51 §5.16 — the shell-private cross-desktop drag preview window
    /// ([`DragPreviewWindow`]). `None` until the first preview-eligible drag
    /// lazily creates it; then persisted (hidden between drags). Kept OUTSIDE
    /// `windows` / `spec_id_to_window_id` so it is invisible to `scene/windows`
    /// (§2 #7) and untouched by the reconcile / dispatch paths.
    drag_preview: Option<DragPreviewWindow<V::Renderer>>,
}

impl<V: WidgetView> AppShell<V> {
    /// R51.76 §5.40 — construct the shell with a freshly-built state
    /// scene and the initial cached state read through the §5.15
    /// introspect channel. Delegates the dispatch substrate to
    /// [`ShellCore::new`]; this constructor only adds the winit /
    /// wgpu / AccessKit surface (`render`, `vello_scene`, `proxy`,
    /// `accesskit`) which lives on `AppShell` itself.
    ///
    /// `proxy` is the same `EventLoopProxy<AppEvent>` the
    /// [`spawn_stdin_rpc_reader`] background thread holds; the shell
    /// retains a clone so it can hand a fresh copy to the
    /// `accesskit_winit::Adapter` on `resumed` (R51.62 §5.40).
    #[must_use]
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        // R999 §5.23 — seed the binding's root Owner with the live
        // `EventLoopProxy`-backed RepaintSink before `ShellCore::new_with_repaint_sink`
        // runs the binding factories, so a binding's `create_extra_externals`
        // can capture it via `use_repaint_sink()` for an off-thread producer.
        let repaint_sink: std::sync::Arc<dyn pinion_core::RepaintSink> =
            std::sync::Arc::new(crate::ProxyRepaintSink::new(proxy.clone()));
        Self {
            core: ShellCore::new_with_repaint_sink(repaint_sink),
            windows: HashMap::new(),
            spec_id_to_window_id: HashMap::new(),
            primary_window_id: None,
            proxy,
            windows_signal: None,
            reconcile_effect: None,
            last_known_specs: Rc::new(RefCell::new(Vec::new())),
            drag_preview: None,
        }
    }

    /// R670.B §5.16 — borrow the primary window's slot, if any.
    /// `None` until `resumed()` has created the primary window. The
    /// primary spec is `V::windows()[0]` (canonically
    /// `WindowSpec::main`); RPC scope default + single-window legacy
    /// paths reach the window through this accessor.
    fn primary_slot(&self) -> Option<&WindowSlot<V::Renderer>> {
        self.primary_window_id.and_then(|id| self.windows.get(&id))
    }

    /// R670.B §5.16 §5.41 — pull the `RenderState` Window arc out of
    /// a slot (active or suspended-with-cached-window). Both render
    /// state variants may carry a `Window` arc; this helper
    /// canonicalises the lookup so callers don't repeat the match.
    fn slot_window(slot: &WindowSlot<V::Renderer>) -> Option<&Arc<Window>> {
        match &slot.render {
            RenderState::Active { window, .. } => Some(window),
            RenderState::Suspended(maybe) => maybe.as_ref(),
        }
    }

    /// R1148 §5.51 §5.16 → R1151 — stamp every live window's ACTUAL client origin
    /// (logical px) into the core so the LIVE cross-window redock resolution maps
    /// the desktop cursor against real desktop positions, not the DECLARED ones — a
    /// WM-placed `"main"` declares position `None` → `(0,0)` but sits at a real WM
    /// offset, which put every floater→main redock off by that offset (the
    /// user-found "좌표 안 맞아" bug). Gated on an active drag this window owns, so
    /// idle hovers + single-window apps skip the `inner_position()` queries. Runs
    /// each `CursorMoved` BEFORE `cursor_moved_for_window` resolves the drop.
    fn stamp_live_window_origins(&self, window_id: WindowId) {
        let Some(spec_id) = self.windows.get(&window_id).map(|s| &*s.spec_id) else {
            return;
        };
        if !self
            .core
            .drag_session_active_for_window(spec_id, PointerId::MOUSE)
        {
            return;
        }
        self.core
            .set_live_window_origins(self.collect_window_origins());
    }

    /// R1149 §5.51 §2 #7 §2 #2 — stamp every live window's ACTUAL outer origin
    /// UNCONDITIONALLY (no drag gate), for the RPC dispatch path. The winit cursor
    /// path stamps in [`Self::stamp_live_window_origins`] (gated on an active
    /// drag), but an RPC `scene/drag` opens the drag DURING its own drain, so the
    /// gate would skip the stamp and the cross-window resolution would fall back to
    /// DECLARED origins (a WM-placed `"main"` at `(0,0)`) — diverging from the live
    /// winit path and making an AI's RPC drive unable to reproduce / diagnose the
    /// live coordinate behavior. Origins are static during a dispatch, so one
    /// unconditional stamp at dispatch entry covers the whole RPC drag drain.
    fn stamp_all_window_origins(&self) {
        self.core
            .set_live_window_origins(self.collect_window_origins());
    }

    /// R1149 §5.51 → R1151 — collect every live window's ACTUAL CLIENT-area origin
    /// in logical pixels (`Window::inner_position()` → logical). Shared by the
    /// winit-path [`Self::stamp_live_window_origins`] and the RPC-path
    /// [`Self::stamp_all_window_origins`] so both resolve cross-window drops
    /// against the same real desktop positions.
    ///
    /// R1151 — `inner_position` (CLIENT top-left), NOT `outer_position` (the
    /// decorated FRAME top-left). The dock scene a cross-window drop hit-tests
    /// against is CLIENT-relative, so a decorated host (e.g. gnome adds a 37px
    /// title bar: `_NET_FRAME_EXTENTS` top=37, so `outer.y = client.y − 37`) made
    /// the hit-test land a title-bar's-worth off — resolving the wrong panel
    /// (a toolbar / the panel's own slot) so the redock fired but did not relocate
    /// ("dropped on the preview, didn't dock"). A borderless floater has no frame,
    /// so `inner == outer` there; this only corrects the decorated host.
    fn collect_window_origins(&self) -> Vec<(String, (f64, f64))> {
        self.windows
            .values()
            .filter_map(|slot| {
                let window = Self::slot_window(slot)?;
                let phys = window.inner_position().ok()?;
                let logical = PhysicalPosition::new(f64::from(phys.x), f64::from(phys.y))
                    .to_logical::<f64>(slot.scale_factor);
                Some((slot.spec_id.to_string(), (logical.x, logical.y)))
            })
            .collect()
    }

    /// R51.38 / R1027 / R1120 §5.35 §5.16 §5.51 — the `CursorMoved` body.
    ///
    /// winit mouse events are single-source on every desktop platform pinion
    /// supports, so the shell threads `PointerId::MOUSE` unconditionally.
    /// `position` is physical; convert to logical so it shares the router's
    /// coordinate space (the same logical space the paint scene lays out in) —
    /// without this a small hit target (splitter handle, slider thumb) is
    /// unreachable on `HiDPI`.
    ///
    /// [`Self::stamp_live_window_origins`] (R1148) runs BEFORE the cursor is
    /// forwarded so an in-flight cross-window redock hit-tests against every live
    /// window's ACTUAL client origin, not the declared one. The `DEFAULT_WINDOW`
    /// fallback mirrors the other pointer arms (an untracked window — a `Resumed`
    /// event landing before the slot is inserted).
    /// R1147 §5.16 — initialise a Vello renderer against `window`'s surface at
    /// its current physical inner size. Shared by [`Self::resume_spec`] (declared
    /// windows) and the R1147 drag-preview window so both cross the §6.3
    /// `pollster::block_on` boundary identically. Returns the boxed renderer or
    /// the backend init error (surface / adapter / device).
    fn build_renderer(
        window: &Arc<Window>,
    ) -> Result<Box<V::Renderer>, <V::Renderer as WidgetRenderer>::Error> {
        let size = window.inner_size();
        pollster::block_on(<V::Renderer as VelloRenderer>::new(
            Arc::clone(window),
            size.width.max(1),
            size.height.max(1),
        ))
        .map(Box::new)
    }

    /// R1147 §5.51 §5.16 — lazily create the shell-private cross-desktop drag
    /// preview window ([`DragPreviewWindow`]), sized to the chip for `label` at
    /// `style`. Borderless + always-on-top + opaque (the window IS the chip),
    /// created HIDDEN so the caller maps it explicitly. Idempotent — a no-op once
    /// built (the window is reused across drags; a later label's resize-to-fit is
    /// [`Self::update_drag_preview`]'s job). Created OUTSIDE `windows` /
    /// `spec_id_to_window_id` / the substrate window registry, so it never
    /// reaches `scene/windows` (§2 #7). The first-ever eligible drag is the only
    /// window creation; subsequent drags reuse it.
    fn ensure_drag_preview_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        label: &str,
        style: pinion_overlay::DragImageStyle,
    ) {
        if self.drag_preview.is_some() {
            return;
        }
        let (w, h) = pinion_overlay::chip_size(label, style);
        let attrs = Window::default_attributes()
            .with_title("pinion-drag-preview")
            .with_decorations(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(f64::from(w.max(1)), f64::from(h.max(1))));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("shell: drag-preview window create failed: {e}");
                return;
            }
        };
        let scale_factor = window.scale_factor();
        let renderer = match Self::build_renderer(&window) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("shell: drag-preview renderer init failed: {e}");
                return;
            }
        };
        let window_id = window.id();
        self.drag_preview = Some(DragPreviewWindow {
            window,
            window_id,
            renderer,
            vello_scene: VelloScene::new(),
            scale_factor,
            visible: false,
            painted_label: None,
        });
    }

    /// R1147 §5.51 §5.16 — paint the drag-preview chip for its current
    /// `painted_label`. The chip scene carries explicit rects (no layout pass),
    /// so it submits straight through [`paint_adapter::to_vello`]. A no-op until a
    /// label is set. Routed from the `RedrawRequested` arm for the preview's
    /// window id; also called directly on a label change / first show so the chip
    /// appears without waiting on a redraw round-trip.
    fn render_drag_preview(&mut self) {
        let Some(preview) = self.drag_preview.as_mut() else {
            return;
        };
        let Some(label) = preview.painted_label.clone() else {
            return;
        };
        let style = V::drag_image_style(&label).unwrap_or_default();
        let (scene, _size) = pinion_overlay::drag_chip_scene(&label, style);
        let base = paint_adapter::root_background(&scene);
        preview.vello_scene.reset();
        // Render-once-per-drag, so a fresh `LayoutCache` per repaint is fine (this
        // is not a per-frame hot path); the chip is single-line single-style text.
        let mut text_cache = pinion_text::LayoutCache::new();
        paint_adapter::to_vello(
            &scene,
            &|_b: &BoxNode| None,
            &mut text_cache,
            &mut preview.vello_scene,
        );
        // R1027 §5.16 — chip is logical; raster at device resolution like every
        // window. The preview repaints rarely, so a local scaled buffer is cheap.
        let scale = preview.scale_factor;
        let _ = if scale_is_non_identity(scale) {
            let mut scaled = VelloScene::new();
            scaled.append(&preview.vello_scene, Some(Affine::scale(scale)));
            submit_frame(&mut *preview.renderer, &scaled, base, false)
        } else {
            submit_frame(&mut *preview.renderer, &preview.vello_scene, base, false)
        };
    }

    /// R1147 §5.51 §5.16 — resize the preview window's GPU surface after a winit
    /// `Resized` (the OS applying the `request_inner_size` a label change issued).
    /// Routed from the `Resized` arm for the preview's window id.
    fn note_preview_resized(&mut self, size: PhysicalSize<u32>) {
        if let Some(preview) = self.drag_preview.as_mut() {
            preview
                .renderer
                .resize(size.width.max(1), size.height.max(1));
            preview.window.request_redraw();
        }
    }

    /// R1147 §5.51 §5.16 — drive the cross-desktop drag preview from a cursor
    /// move. On a preview-eligible drag (the binding opted the dragged label in
    /// via [`WidgetView::drag_image_style`]) this ensures the window, repaints the
    /// chip iff the label changed (render-once), positions it at the DESKTOP
    /// cursor (the SOURCE window's ACTUAL client origin + the window-local cursor —
    /// the live origin, not the lagging declared one; the source window is the
    /// dragger, not the moved one, so there is no R1119 feedback loop), shows it,
    /// and toggles the in-window-overlay suppression. Skipped under
    /// `PINION_HIDDEN_WINDOW` (headless RPC runs keep the R1113 in-window overlay
    /// as the introspection chip rather than flashing a real window).
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "logical-pixel desktop coords + chip dims -> integer window geometry; sub-pixel rounding + sign are irrelevant (origin/cursor desktop coords, chip dims clamped >= 1)"
    )]
    fn update_drag_preview(
        &mut self,
        window_id: WindowId,
        cursor: (f64, f64),
        event_loop: &ActiveEventLoop,
    ) {
        if hidden_window_requested() {
            return;
        }
        let Some(spec_id) = self.windows.get(&window_id).map(|s| s.spec_id.clone()) else {
            return;
        };
        let Some((label, _)) = self
            .core
            .active_drag_label_for_window(&spec_id, PointerId::MOUSE)
        else {
            // No active drag for this window: the release path hides any preview.
            return;
        };
        if label.is_empty() {
            return;
        }
        let Some(style) = V::drag_image_style(&label) else {
            // The binding wants no follower for this label.
            return;
        };
        // Desktop pointer = the SOURCE window's ACTUAL CLIENT origin + the
        // window-local (client-relative) cursor, trailed by the style offset so the
        // chip sits beside the pointer (matching the in-window R1113 follower).
        // Reads the SAME live-window-origin SSOT the cross-window redock stamps each
        // move (R1148/R1151): `stamp_live_window_origins` runs just above in
        // `handle_cursor_moved`, so the source's origin is fresh. CLIENT (not outer)
        // is the correct base — winit reports the cursor client-relative, so
        // `client_origin + client_cursor` is the true desktop pointer even on a
        // decorated source (a borderless floater has client == outer).
        let Some((ox, oy)) = self.core.live_window_origin(&spec_id) else {
            return;
        };
        let off = i32::try_from(style.cursor_offset).unwrap_or(i32::MAX);
        let pos = (
            (ox + cursor.0).round() as i32 + off,
            (oy + cursor.1).round() as i32 + off,
        );
        self.ensure_drag_preview_window(event_loop, &label, style);
        let repaint = {
            let Some(preview) = self.drag_preview.as_mut() else {
                return;
            };
            let label_changed = preview.painted_label.as_deref() != Some(label.as_str());
            if label_changed {
                let (w, h) = pinion_overlay::chip_size(&label, style);
                let _ = preview
                    .window
                    .request_inner_size(LogicalSize::new(f64::from(w.max(1)), f64::from(h.max(1))));
                // Resize the surface NOW so this frame rasters at the new size;
                // the matching `Resized` event re-applies idempotently.
                let s = preview.scale_factor;
                let pw = (f64::from(w.max(1)) * s).round() as u32;
                let ph = (f64::from(h.max(1)) * s).round() as u32;
                preview.renderer.resize(pw.max(1), ph.max(1));
                preview.painted_label = Some(label.clone());
            }
            preview
                .window
                .set_outer_position(LogicalPosition::new(f64::from(pos.0), f64::from(pos.1)));
            let became_visible = !preview.visible;
            if became_visible {
                preview.window.set_visible(true);
                preview.visible = true;
            }
            label_changed || became_visible
        };
        // Suppress the in-window overlay (one chip, not two) + repaint the source
        // window once so the in-window ghost is stripped now.
        self.core.set_desktop_drag_preview_active(true);
        if repaint {
            self.render_drag_preview();
            self.core.request_redraw();
        }
    }

    /// R1147 §5.51 §5.16 — hide the drag preview on drag end (release / cancel).
    /// `set_visible(false)` keeps the window for reuse; clears the suppression
    /// flag so the in-window overlay resumes (e.g. for headless introspection).
    fn hide_drag_preview(&mut self) {
        if let Some(preview) = self.drag_preview.as_mut()
            && preview.visible
        {
            preview.window.set_visible(false);
            preview.visible = false;
        }
        self.core.set_desktop_drag_preview_active(false);
    }

    /// R1147 §5.51 §5.16 — whether `window_id` is the shell-private drag-preview
    /// window (so `window_event` routes its `RedrawRequested` / `Resized` to the
    /// preview's own render path instead of the declared-window `windows` map).
    fn is_drag_preview_window(&self, window_id: WindowId) -> bool {
        self.drag_preview
            .as_ref()
            .is_some_and(|p| p.window_id == window_id)
    }

    fn handle_cursor_moved(
        &mut self,
        window_id: WindowId,
        position: PhysicalPosition<f64>,
        scale: f64,
        event_loop: &ActiveEventLoop,
    ) {
        let (lx, ly) = winit_pointer_to_logical(position, scale);
        // R1148/R1151 §5.51 — stamp every live window's ACTUAL client origin
        // BEFORE the resolution below, so a floater→main redock hit-tests against
        // real desktop positions (a WM-placed `"main"` has no declared position).
        self.stamp_live_window_origins(window_id);
        let spec_id: &str = self
            .windows
            .get(&window_id)
            .map_or(pinion_runtime::DEFAULT_WINDOW, |s| &*s.spec_id);
        self.core
            .cursor_moved_for_window(spec_id, PointerId::MOUSE, lx, ly);
        // R1147 §5.51 — drive the cross-desktop drag preview window (a no-op
        // unless a preview-eligible drag is in flight in live mode).
        self.update_drag_preview(window_id, (lx, ly), event_loop);
    }

    /// R51.76 §5.40 — drain the [`ShellCore::redraw_requested`] flag
    /// and forward to every live winit `Window`.
    ///
    /// R670.B §5.16 — pre-R670.B forwarded to the single window only;
    /// post-R670.B forwards to every active window slot so a
    /// multi-window binding gets all windows repainted on a single
    /// state change (the canonical "main button click → inspector
    /// state mirror updates" arc requires this). Steady-state
    /// single-window bindings are unaffected (one window in the
    /// hashmap = one `request_redraw` call).
    ///
    /// Called at the end of every `window_event` / `user_event`
    /// `ApplicationHandler` arm so all redraw requests collapse into a
    /// single `Window::request_redraw` per window per
    /// event-loop iteration.
    ///
    /// R1023.1 §5.16 — the two redraw-request idioms, and when each applies:
    /// - **Ledger route** (this drain): triggers that cannot name their window
    ///   up front — state mutations, RPC dispatch, animation ticks, external
    ///   repaints — set [`ShellCore::request_redraw`] /
    ///   [`ShellCore::request_redraw_for_window`] and rely on this chokepoint to
    ///   coalesce + forward. This is the default and the coalescing SSOT.
    /// - **Direct poke**: winit-event arms that already hold the `Arc<Window>`
    ///   and need to repaint exactly that surface — the `WindowEvent::Resized`
    ///   arm (R1023) and the `request_inner_size` sites — call
    ///   `window.request_redraw()` directly. winit itself coalesces repeated
    ///   `request_redraw` before the next `RedrawRequested`, so the direct poke
    ///   and the ledger converge to one paint per frame; the direct form just
    ///   skips a ledger round-trip when the window is already in hand.
    fn drain_redraw_to_winit(&mut self) {
        // R680 atomic 2 §5.16 §5.41 — two-tier redraw drain:
        // - Binding-wide [`ShellCore::redraw_requested`] flag fans
        //   out to every active window slot (the pre-R680 contract,
        //   safe default for state mutations that cannot reliably
        //   attribute themselves to a single window).
        // - Per-window [`ShellCore::redraw_requested_per_window`]
        //   flags target individual slots (R680 atomic 3+ RPC
        //   dispatch follow-ups, R681 immediate-mode subtree
        //   polling, R683 dock-panel local layout reactions). The
        //   binding-wide drain takes priority — if the fan-out
        //   flag was set, every slot's per-window flag is also
        //   considered "satisfied" by the same `request_redraw`
        //   call, so we drain (clear) the per-window flag too to
        //   avoid a spurious follow-up wake-up in the next
        //   event-loop iteration.
        let fan_out = self.core.take_redraw_request();
        // Collect spec_ids first so the per-window drain doesn't
        // hold a `&self.windows` borrow across the
        // `self.core.take_redraw_request_for_window` `&mut self.core`
        // mutation.
        //
        // R683 §5.16 — `spec_id: Cow<'static, str>` because the dock
        // + tear-off arc can mint runtime-generated ids alongside
        // static literals; the `.clone()` is `Rc`-cheap for
        // `Cow::Borrowed` (no heap alloc) and a `String::clone` for
        // `Cow::Owned`.
        let active: Vec<(Cow<'static, str>, std::sync::Arc<Window>)> = self
            .windows
            .values()
            .filter_map(|slot| {
                if let RenderState::Active { window, .. } = &slot.render {
                    Some((slot.spec_id.clone(), std::sync::Arc::clone(window)))
                } else {
                    None
                }
            })
            .collect();
        for (spec_id, window) in active {
            // Always drain the per-window flag so a stale `true`
            // does not survive a binding-wide fan-out drain.
            let per_window = self.core.take_redraw_request_for_window(&spec_id);
            if fan_out || per_window {
                window.request_redraw();
            }
        }
    }

    /// R51.76 §5.40 — thin wrapper around [`ShellCore::dispatch_rpc`]
    /// that builds the production `resize_request` closure from the
    /// live winit `Window` and writes the JSON-RPC response to
    /// stdout. Headless tests call `ShellCore::dispatch_rpc` directly
    /// with a no-op closure.
    fn dispatch_rpc(&mut self, request: &str) {
        // R671 §5.7 §5.16 — single-parse per-window RPC dispatch.
        // Pre-R671 (R670.B) AppShell parsed the JSON-RPC envelope
        // *twice*: once to sniff `params.window` (the per-window
        // scope) + once inside `pinion_rpc::dispatch` for actual
        // routing. R671 parses once via `pinion_rpc::parse_request`
        // + extracts the window scope from `Request.params` + hands
        // the same `Request` to the substrate which forwards to
        // `pinion_rpc::dispatch_parsed`. Parse errors short-circuit
        // here + we write the canonical -32700 frame to stdout.
        let parsed_request = match pinion_rpc::parse_request(request) {
            Ok(r) => r,
            Err(err_resp) => {
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{err_resp}");
                return;
            }
        };
        // R1149 §5.51 §2 #7 §2 #2 — stamp every window's ACTUAL outer origin so a
        // cross-window resolution during this RPC (a `scene/drag` redock) uses the
        // SAME actual origins the live winit path does. Without it the RPC drain
        // fell back to DECLARED origins (WM-placed `"main"` at `(0,0)`), so an AI's
        // RPC drive could NOT reproduce — let alone diagnose — a live coordinate
        // divergence. Multi-window only (single-window never resolves cross-window).
        if self.windows.len() > 1 {
            self.stamp_all_window_origins();
        }
        // R890.1 §5.16 §5.49 — no window extraction here at all: the
        // substrate's windowed entry derives the dispatch scope from
        // the request's own `{window: "<id>"}` param through the ONE
        // extraction home (`pinion_rpc::Request::window_scope`), and
        // the dispatcher gates unknown ids with `-32602
        // unknown_window` before method routing. Pre-R889 this site
        // hosted `resolve_spec_id` (silent alias of unknown ids onto
        // the primary); pre-R890.1 it still hand-rolled a second copy
        // of the param extraction that had to agree with the gate's
        // forever.
        // R670.B §5.16 — primary-window-scoped `scene/resize` (the
        // resize closure still targets the primary window; per-window
        // `scene/resize` is a follow-up axis once a real consumer
        // surfaces — typical multi-window app resizes the main
        // window, not the inspector). Holding the Arc<Window> across
        // the substrate call keeps the closure's `&Arc<Window>`
        // borrow alive without re-borrowing `self.windows` from
        // inside the closure.
        let primary_window_arc: Option<Arc<Window>> =
            self.primary_slot().and_then(Self::slot_window).cloned();
        let mut resize_req = |w: u32, h: u32| {
            // R47.7.4.2 — `scene/resize` reaches winit through this
            // closure: `request_inner_size` queues a size change that
            // winit emits as a `Resized` event on the next loop pass,
            // and the explicit `request_redraw` shortens the gap to
            // the new paint scene observation.
            if let Some(window) = &primary_window_arc {
                let _ = window.request_inner_size(LogicalSize::new(w, h));
                window.request_redraw();
            }
        };
        // R890 §5.12 §5.16 — no slot-layout threading either: the
        // dispatcher projects the addressed window's layout on demand
        // from the stored paint scene it already threads for
        // `scene/snapshot from: paint`, so `scene/layout {viewport:
        // null}` answers with the named window's own geometry or the
        // honest `NoLastPaintLayout`.
        // R1060 §5.12 §5.16 — a `scene/screenshot` request reads the
        // live presented surface, which only AppShell (not ShellCore)
        // can reach. Capture it here, before the `&mut self.core`
        // dispatch borrow, when the method matches; every other method
        // skips the GPU readback (the render_fidelity snapshot pattern,
        // lazily gated by method so non-screenshot dispatches pay
        // nothing). `scope` borrows `parsed_request`, so the capture runs
        // before `parsed_request` moves into the dispatch.
        let screenshot = if parsed_request.method == "scene/screenshot" {
            let scope = parsed_request.window_scope().ok().flatten();
            // R1061 §5.12 — optional `{out_path: "….png"}` switches the
            // wire to file output (small response) vs the inline
            // `pixels_rgba8` array (default).
            let out_path = parsed_request
                .params
                .as_ref()
                .and_then(|p| p.get("out_path"))
                .and_then(serde_json::Value::as_str);
            self.capture_window_screenshot(scope, out_path)
        } else {
            None
        };
        let resp = self
            .core
            .dispatch_rpc_scoped(parsed_request, &mut resize_req, screenshot);
        if let Some(resp) = resp {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{resp}").is_err() {
                // stdout closed (downstream consumer gone) — silently
                // skip; do not abort the GUI loop on a broken pipe.
            }
        }
    }

    /// R1060 §5.12 §5.16 — capture the addressed window's live presented
    /// surface for a `scene/screenshot` RPC. Resolves the request's
    /// `{window: "<id>"}` scope (absent → the primary window) to a slot,
    /// flags it for capture, drives ONE [`Self::render_window`] pass (which
    /// then submits through [`VelloRenderer::capture_rgba8`], reading back
    /// the swapchain texture instead of presenting blind), then drains +
    /// converts the frame to the wire [`pinion_rpc::Screenshot`].
    ///
    /// Returns `None` when the window is unknown or the GPU capture
    /// failed; the dispatcher then surfaces `unknown_window` (its own
    /// gate) or `RenderBackendUnavailable` (the screenshot handler's
    /// absent-snapshot path). This is the ONE site that can read live
    /// pixels — the dispatch runs in `ShellCore`, which holds no renderer.
    fn capture_window_screenshot(
        &mut self,
        window_scope: Option<&str>,
        out_path: Option<&str>,
    ) -> Option<pinion_rpc::Screenshot> {
        let window_id = match window_scope {
            Some(spec_id) => self.spec_id_to_window_id.get(spec_id).copied(),
            None => self.primary_window_id,
        }?;
        self.windows.get_mut(&window_id)?.pending_capture = true;
        self.render_window(window_id);
        // R1062 §5.12 — clear the one-shot flag UNCONDITIONALLY: if
        // `render_window` early-returned (minimized / 0-size or suspended
        // window) it never consumed the flag, and a stale `true` would
        // make the NEXT event-loop paint capture by surprise — a
        // multi-MB readback + a blocking `device.poll` on the 60-144fps
        // hot path, plus an orphaned frame. The capture still fails
        // honestly via the `last_capture.take()?` below.
        let slot = self.windows.get_mut(&window_id)?;
        slot.pending_capture = false;
        let frame = slot.last_capture.take()?;
        // R1061 §5.12 — `{out_path}` mode: write the captured frame to the
        // file as PNG (the RGBA8 -> PNG SSOT shared with the headless
        // path) and return just the path, so the wire stays small for
        // large windows. A create / encode failure fails the capture
        // (`None` -> `RenderBackendUnavailable`) rather than returning a
        // wrong frame. This filesystem write is the ONE side-effect on
        // the otherwise-Read `scene/screenshot` method — scene + OCC
        // state are untouched, so the `HandlerKind::Read` classification
        // holds; the write is the client's explicitly-requested output
        // (same trust model as the `PINION_SCREENSHOT` env path).
        if let Some(path) = out_path {
            let file = std::fs::File::create(path).ok()?;
            crate::vello_capture::encode_rgba8_png(frame.width, frame.height, &frame.rgba8, file)
                .ok()?;
            return Some(pinion_rpc::Screenshot::new_file(
                frame.width,
                frame.height,
                path.to_owned(),
            ));
        }
        Some(pinion_rpc::Screenshot::new(
            frame.width,
            frame.height,
            frame.rgba8,
        ))
    }

    /// R670.B §5.16 — per-window AccessKit emit helper. Extracted
    /// from `render_window` so the parent fn stays under the
    /// workspace `clippy::too_many_lines = 100` ceiling after the
    /// per-window cluster lift R670.B added. The body is unchanged
    /// from the pre-R670.B inline version; only the slot lookup +
    /// disjoint-borrow split (slot.accesskit, slot vs self.core)
    /// differ.
    fn emit_accesskit_for_window(
        &mut self,
        window_id: WindowId,
        spec_id: &str,
        paint_scene: &pinion_core::Scene,
        size_w: u32,
        size_h: u32,
        scale_factor: f64,
    ) {
        let Some(slot) = self.windows.get_mut(&window_id) else {
            return;
        };
        if slot.accesskit.is_none() {
            return;
        }
        // R813 §5.40 — thread the resolved spec id so the substrate calls
        // `V::access_node_for_window(spec_id, ...)`: each window's AT tree
        // carries only its own nodes (no cross-window ghost nodes).
        let (nodes, at_focus) = self.core.collect_access_emit_inputs(spec_id, paint_scene);
        let window_bounds = pinion_core::scene::Rect::new(0, 0, size_w, size_h);
        let decision = self.core.plan_access_emit(&nodes, at_focus.as_ref());
        // Re-acquire the slot mutable borrow now that the substrate
        // borrows released (collect_access_emit_inputs +
        // plan_access_emit take `&mut self.core`).
        let Some(slot) = self.windows.get_mut(&window_id) else {
            return;
        };
        if decision.should_emit
            && let Some(adapter) = slot.accesskit.as_mut()
        {
            let initial = decision.initial;
            let dirty = decision.dirty;
            let at_focus_ref = at_focus.as_ref();
            let nodes_ref = &nodes;
            adapter.update_if_active(|| {
                let mut builder = AccessTreeBuilder::new();
                if !initial {
                    builder.initial(false);
                }
                for node in nodes_ref {
                    builder.add(node);
                }
                if !initial {
                    builder.dirty_tags(dirty);
                }
                if let Some(f) = at_focus_ref {
                    builder.focused(Some(&f.focus_tag));
                    if let Some(child) = &f.active_descendant {
                        builder.active_descendant(&f.focus_tag, child);
                    }
                } else {
                    builder.focused(None);
                }
                // R1027 §5.40 — `window_bounds` + every AccessNode rect
                // are logical pixels (the paint scene is logical since
                // R1027). The root-node `Affine::scale(scale_factor)`
                // re-expresses the whole tree in the physical-pixel space
                // AccessKit expects; identity scale leaves it byte-identical.
                builder.build_with_scale(Some(window_bounds), scale_factor)
            });
        }
        // R51.77 / R51.79 §5.40 — commit step. By-value Vec move
        // into the cache; nodes consumed here. Idempotent on
        // !should_emit so the next frame's plan diffs against the
        // post-emit baseline.
        self.core.commit_access_emit(nodes, at_focus.as_ref());
    }

    /// Build the paint scene for the current cached state, run layout,
    /// hand it to the framework-side `paint_adapter` walker, and submit
    /// the resulting `vello::Scene` to the renderer. No-op while
    /// suspended (R46.3.4 lifecycle).
    ///
    /// R51.76 §5.40 — the AccessKit emit decision is delegated to
    /// [`ShellCore::compute_access_emit`] so the same diff logic is
    /// exercised by headless tests; the AppShell-side responsibility
    /// is just to feed the plan to `Adapter::update_if_active`.
    /// R670.B §5.16 §5.41 — render one window's paint scene.
    ///
    /// Pre-R670.B was the single-window `fn render(&mut self)`; the
    /// body is unchanged structurally — only the field accesses now
    /// resolve through `self.windows[&window_id]` instead of
    /// `self.render` / `self.accesskit` / `self.vello_scene` /
    /// `self.last_ime_cursor_area` / `self.pending_intrinsic_resize`.
    /// The §5.16 §5.36 substrate ownership of the paint scene
    /// pipeline (`ShellCore::compute_paint_scene`) is unchanged — the
    /// surface still only handles the vello/wgpu submit per window.
    ///
    /// No-op when the slot is missing (window already dropped) or
    /// when its [`RenderState`] is `Suspended` (GPU released, mobile
    /// platform cycle). Mirrors the pre-R670.B `let RenderState::
    /// Active { .. } = &mut self.render else { return; }` guard.
    fn render_window(&mut self, window_id: WindowId) {
        // R670.B §5.16 — read the slot's spec id + inner size first
        // without holding a long-lived `&mut self.windows` borrow,
        // so the subsequent `self.core.compute_paint_scene_for_window`
        // call (which takes `&mut self.core`) does not conflict with
        // the slot borrow needed to call back into vello_scene /
        // accesskit / last_ime_cursor_area / pending_intrinsic_resize
        // after the paint scene is computed.
        // R683 §5.16 — `spec_id: Cow<'static, str>` so the dock +
        // tear-off arc can mint runtime ids; `.clone()` here is
        // `Cow`-cheap for `Borrowed` (no heap alloc) and a
        // `String::clone` for `Owned`. The clone detaches from the
        // `&slot` borrow so the substrate's `&mut self.core` call
        // below does not conflict.
        let (spec_id, scale, w, h) = {
            let Some(slot) = self.windows.get(&window_id) else {
                return;
            };
            let RenderState::Active { window, .. } = &slot.render else {
                return;
            };
            // R1027 §5.16 — lay out in logical pixels. winit reports the
            // surface in physical pixels; the window `scale_factor` maps
            // physical -> logical so app-authored dimensions render at
            // their intended size on `HiDPI` (the scale is re-applied at the
            // GPU raster boundary below). `logical_layout_size` may return a
            // `0` dimension for a degenerate (minimized / sub-pixel) window;
            // the `NonZeroU32` guards below then early-return with no paint —
            // the load-bearing 0-size skip pre-R1027 got from feeding the raw
            // physical `inner_size` to `NonZeroU32::new`.
            let scale = slot.scale_factor;
            let (lw, lh) = logical_layout_size(window.inner_size(), scale);
            let Some(w) = core::num::NonZeroU32::new(lw) else {
                return;
            };
            let Some(h) = core::num::NonZeroU32::new(lh) else {
                return;
            };
            (slot.spec_id.clone(), scale, w, h)
        };
        // R51.80 §5.16 §5.36 — ShellCore owns the paint scene
        // pipeline; AppShell only handles the vello/wgpu submit.
        // R670.B §5.16 — `compute_paint_scene_for_window` is the
        // per-window variant that routes through
        // `V::view_for_window(spec_id, state, frame)`; default impl
        // forwards to `V::view` so single-window bindings remain
        // bit-identical.
        // R683 §5.16 — `&spec_id` auto-derefs from
        // `&Cow<'static, str>` to `&str` through `Cow`'s `Deref`
        // impl, so the substrate signature (which takes `&str`)
        // stays unchanged.
        // R907 §5.16 §5.7 — frame-timing profiler: bracket the whole
        // productive frame (build → finalize) and each named phase with
        // `Instant` spans. `build_us` is the `view` + layout pass; the
        // scope below measures `encode_us` (to_vello_cached) and
        // `render_us` (GPU submit) into these vars; `total_us` closes
        // after finalize. `total >= build + encode + render` holds by
        // construction (disjoint sub-intervals).
        let frame_start = Instant::now();
        let paint_scene = self
            .core
            .compute_paint_scene_for_window(&spec_id, w.get(), h.get());
        let build_us = instant_delta_us(frame_start, Instant::now());
        // R668 §5.16 / R1072.1 — `IntrinsicAfterFirstPaint` resize hook, extracted
        // into `apply_pending_intrinsic_resize`: walk the first painted scene for
        // its content size, clamp to `[min, max]`, request the new winit inner-size
        // (applied next frame via `Resized`). Runs BEFORE the encode so the encode
        // uses the current size; a no-op for `Fixed` strategy + steady-state paints.
        self.apply_pending_intrinsic_resize(window_id, &paint_scene, w, h);
        // Assigned exactly once inside the paint scope below; the
        // scope's `else { return; }` arms diverge, so the fall-through
        // path that reaches `record_frame_timing` always assigns both.
        let encode_us;
        let render_us;
        // R1036 PR-17 — the `renderer.render` outcome for this frame, fed into
        // the per-window render-fidelity record so `scene/render_fidelity`
        // surfaces a failed present (the present-staleness signature).
        let present_ok;
        // Re-acquire the slot mutable borrow now that the substrate
        // borrow released, then bind window + renderer for the
        // intrinsic-resize hook + vello submit. Scope the borrow
        // so it drops before the post-paint helpers
        // (emit_accesskit_for_window, publish_ime_for_window) which
        // take `&mut self`.
        // R1027 §5.16 — this scope no longer yields the physical
        // `inner_size`: AccessKit bounds are now logical (`w`/`h`) and the
        // GPU surface is sized by the `Resized` arm, so nothing past the
        // paint needs the physical size.
        {
            let Some(slot) = self.windows.get_mut(&window_id) else {
                return;
            };
            let RenderState::Active { renderer, .. } = &mut slot.render else {
                return;
            };
            slot.vello_scene.reset();
            let base = paint_adapter::root_background(&paint_scene);
            // R682 §5.16 atomic 1 — cached path. The per-window
            // `FragmentCache` skips re-encoding cacheable Container
            // subtrees whose `paint_hash` matches the previous frame
            // (the §2 #4 immediate-mode coexistence enabler: a sibling
            // `ImmediateModeNode` triggers `V::view` re-runs every paint,
            // but retained widget subtrees with stable structure replay
            // their encoded `vello::Scene` via `append` instead of fresh
            // walk). `&|_b| None` is the canonical no-override fill hook
            // every production shell call site passes — the cache's
            // structurally-derived contract holds trivially.
            // R705 §5.39 — the focus ring is no longer stroked here. It is
            // injected upstream as a pointer-transparent overlay
            // `Scene::Box` by `Substrate::apply_focus_ring` (the final step
            // of every paint-scene producer), so `to_vello_cached` paints it
            // via the generic box path and `scene/snapshot from: paint`
            // observes it (§2 #1 + #7). The pre-R705 opaque
            // `paint_adapter::paint_focus_ring` vello stroke is retired.
            let encode_start = Instant::now();
            // R1072 §5.37 — engine-aware cached paint: cache (mut) + opt-in engine
            // (shared) from one disjoint-field borrow. `None` = pre-R1072 path.
            let (text_cache, text_engine) = self.core.text_cache_and_engine();
            paint_adapter::to_vello_cached_with_text_engine(
                &paint_scene,
                &|_b: &BoxNode| None,
                text_cache,
                &mut slot.image_cache,
                &mut slot.fragment_cache,
                text_engine,
                &mut slot.vello_scene,
            );
            encode_us = instant_delta_us(encode_start, Instant::now());
            // R51.109.1 §5.41 — call through the backend-agnostic
            // `WidgetRenderer` trait. `VelloContext::base_color` carries
            // the window background sampled from
            // `paint_adapter::root_background`; the renderer's macro impl
            // forwards to the inherent `<R>::render(frame, base_color)`.
            // `renderer.render` auto-derefs through `Box<R>` because the
            // `WidgetRenderer` trait is in scope.
            let render_start = Instant::now();
            // R1027 §5.16 — the paint scene (and thus `vello_scene`) is in
            // logical pixels; the GPU surface is physical. At non-identity
            // scale, append the logical scene into `scaled_scene` under
            // `Affine::scale(scale)` so it rasterizes at device resolution.
            // The `FragmentCache` stays IDENTITY-keyed and fully intact —
            // the scale is one top-level transform applied AFTER the cached
            // walk, not threaded through it, so cache hits are unaffected.
            // At `1.0` the renderer submits `vello_scene` directly: zero
            // extra append, byte-identical to the pre-R1027 output.
            let render_target: &VelloScene = if scale_is_non_identity(scale) {
                slot.scaled_scene.reset();
                slot.scaled_scene
                    .append(&slot.vello_scene, Some(Affine::scale(scale)));
                &slot.scaled_scene
            } else {
                &slot.vello_scene
            };
            // R1060 §5.16 §5.12 — submit the frame: the normal present,
            // or — when an RPC `scene/screenshot` flagged this slot via
            // `capture_window_screenshot` — `capture_rgba8` reading back
            // the presented swapchain. The flag is false on every
            // event-loop-driven paint, so the hot path is byte-identical.
            // R1062 — read-only here: `capture_window_screenshot` OWNS the
            // flag's set+clear lifecycle (it clears unconditionally after
            // this call, so an early-return path above cannot leave it
            // stale and make a later event-loop paint capture by surprise).
            let wants_capture = slot.pending_capture;
            let captured;
            (present_ok, captured) =
                submit_frame(&mut **renderer, render_target, base, wants_capture);
            if let Some(frame) = captured {
                slot.last_capture = Some(frame);
            }
            render_us = instant_delta_us(render_start, Instant::now());
        };
        // R1036 PR-17 §2 #7 — record the uncontaminated fidelity fingerprint of
        // the frame just ENCODED + presented for this window (per-TextGrid
        // used-row count + content hash + present outcome). Written ONLY here on
        // the winit paint path, never by an RPC recompute, so
        // `scene/render_fidelity` can answer "what is actually displayed"
        // without the `last_paint_scene` post-dispatch-finalize contamination.
        self.core
            .record_presented_frame(&spec_id, present_ok, (w.get(), h.get()), &paint_scene);
        // The post-paint helpers (`emit_accesskit_for_window`,
        // `publish_ime_for_window`) each re-acquire their own slot
        // borrow internally and take `&mut self.core` — the scope
        // above released the long-held slot borrow so this is safe.
        // R1027 §5.16 §5.40 — AccessKit window bounds are the LOGICAL
        // (`w`, `h`) dims (matching the logical paint scene the AccessNode
        // rects are collected from); `scale` rides through to the root
        // node transform so the AT side still sees physical-pixel coords.
        self.emit_accesskit_for_window(window_id, &spec_id, &paint_scene, w.get(), h.get(), scale);
        // R56.2.c §5.13 §5.38 — push IME candidate window position
        // to the platform IME (per-window since R670.B).
        self.publish_ime_for_window(window_id, &paint_scene);
        // R890 §5.12 §5.16 — no per-frame [`pinion_rpc::LayoutNode`]
        // build any more: the paint scene `finalize_frame_for_window`
        // stores in the addressed window's router IS the layout
        // source; the substrate projects it on demand at the AI-paced
        // RPC read (`ShellCore::last_paint_layout_for_window`). The
        // R671-era per-frame build + per-slot clone paid an O(painted
        // tree) walk on EVERY winit frame for data only RPC consumed.
        // R683 §5.16 — `spec_id` is `Cow<'static, str>`; clone for
        // the post-borrow `finalize_frame_for_window` call (clones
        // are `Cow`-cheap for `Borrowed`, `String::clone` for
        // `Owned`).
        let spec_id_for_finalize = self.windows.get(&window_id).map(|s| s.spec_id.clone());
        // R682 §5.16 atomic 3 — capture the post-paint cache
        // snapshot before publishing. `to_vello_cached` brackets its
        // walk with begin_paint / end_paint internally, so the
        // counters / damage region read here are the post-sweep
        // publishable snapshot. Computed before the optional slot
        // borrow below + the publish call so neither the slot
        // borrow nor the `&mut self.core` publish call conflict.
        let cache_stats = self
            .windows
            .get(&window_id)
            .map(|slot| slot.fragment_cache.stats());
        // R681 §2 #4 atomic 2 — capture the immediate-mode pacing flag
        // before `paint_scene` is moved into `finalize_frame_for_window`
        // below. Published to the substrate after finalize (keyed by the
        // finalize target window, like `record_frame_timing`), so pacing
        // and the §5.16 jank profiler read it from one home.
        let has_immediate_subtree = paint_scene.has_immediate_mode_subtree();
        // R682 §5.16 atomic 3 — publish the snapshot into the
        // GUI-agnostic substrate so RPC + tests can introspect cache
        // observability without depending on the
        // vello::Scene-bearing FragmentCache directly. Skip when the
        // spec_id is unknown (defensive — the slot borrow above
        // would have already returned in that case).
        //
        // R683 §5.16 — `spec_id_for_finalize` is now
        // `Option<Cow<'static, str>>`; `as_deref()` projects to
        // `Option<&str>` so both substrate signatures (which take
        // `&str`) consume the same borrow shape they had under the
        // pre-R683 `&'static str` field type. `unwrap_or` returns
        // `&str` directly because `pinion_runtime::DEFAULT_WINDOW`
        // is still a `&'static str`.
        let target_window: &str = spec_id_for_finalize
            .as_deref()
            .unwrap_or(pinion_runtime::DEFAULT_WINDOW);
        if let Some(stats) = cache_stats {
            self.core.publish_fragment_cache_stats(target_window, stats);
        }
        // R51.80 §5.12 §5.35 — hand the rendered scene to the
        // substrate. `finalize_frame` refreshes the input router +
        // intent drain in one method; the stored scene doubles as the
        // `scene/layout` source (R890).
        //
        // R672 §5.35 §5.41 — route through `finalize_frame_for_window`
        // so the addressed window's [`pinion_runtime::InputRouter`]
        // (not the binding-wide single router pre-R672) sees the
        // paint scene. Each window's pointer state stays isolated;
        // cross-window paint cycles no longer flip-flop hover state.
        self.core
            .finalize_frame_for_window(target_window, paint_scene);
        // R907 §5.16 §5.7 — close the total-frame span (after finalize)
        // and record the sample into the per-window rolling profiler
        // window. The O(window) aggregate fold is deferred to the
        // AI-paced `scene/frame_timings` read, never run here.
        let total_us = instant_delta_us(frame_start, Instant::now());
        self.core.record_frame_timing(
            target_window,
            pinion_runtime::FrameTiming::new(build_us, encode_us, render_us, total_us),
        );
        // R681 §2 #4 atomic 2 — publish the sticky immediate-mode flag
        // into the substrate (one home with `target_fps`). The next
        // `about_to_wait` reads it to choose `ControlFlow::Wait` vs
        // `WaitUntil(deadline)`, and the §5.16 jank profiler derives the
        // same frame budget from it — pacing and observability cannot
        // disagree because they read this one signal.
        self.core
            .set_immediate_subtree_for_window(target_window, has_immediate_subtree);
    }

    /// R668 §5.16 / R1072.1 — the `IntrinsicAfterFirstPaint` post-first-paint
    /// resize hook, extracted from [`Self::render_window`] (the same discipline
    /// `publish_ime_for_window` / `emit_accesskit_for_window` follow to keep the
    /// parent under the `clippy::too_many_lines = 100` ceiling — preferred over a
    /// lint suppression).
    ///
    /// The first painted scene carries layout-computed rects on every node, so
    /// walking the tree ([`Scene::intrinsic_content_size`]) gives the tight
    /// `(width, height)` the content wants; clamp to `[min, max]` and forward to
    /// [`Window::request_inner_size`]. winit emits a `WindowEvent::Resized` on
    /// acceptance which re-enters the layout pass at the new viewport next paint.
    /// Self-draining: `Fixed`-strategy paints and every steady-state paint after
    /// the first take the `None` branch and no-op. A vanished slot / non-`Active`
    /// render state is also a no-op.
    fn apply_pending_intrinsic_resize(
        &mut self,
        window_id: WindowId,
        paint_scene: &pinion_core::Scene,
        w: core::num::NonZeroU32,
        h: core::num::NonZeroU32,
    ) {
        let Some(slot) = self.windows.get_mut(&window_id) else {
            return;
        };
        let RenderState::Active { window, .. } = &mut slot.render else {
            return;
        };
        let Some((min, max)) = slot.pending_intrinsic_resize.take() else {
            return;
        };
        let (content_w, content_h) = paint_scene.intrinsic_content_size();
        let target_w = content_w.clamp(min.0, max.0);
        let target_h = content_h.clamp(min.1, max.1);
        if (target_w, target_h) != (w.get(), h.get()) {
            let _ = window
                .request_inner_size(LogicalSize::new(f64::from(target_w), f64::from(target_h)));
            // Force-request a redraw so the next event-loop pass re-enters
            // `render` against the updated inner_size and paints the final
            // layout immediately rather than idling on the now-undersized
            // first-paint frame.
            window.request_redraw();
        }
    }

    /// R670.B §5.16 — per-window IME candidate publish helper.
    /// Extracted from `render_window` so the parent fn stays under
    /// the workspace `clippy::too_many_lines = 100` ceiling after
    /// the per-window cluster lift R670.B added.
    ///
    /// Runs `V::ime_caret_rect` inside the substrate's root-owner
    /// scope so application hooks (`use_text_edit_state(tag)` /
    /// `use_layout_cache(key)`) resolve through `Owner::current()`
    /// inside the trait call. Dedups against the slot's
    /// `last_ime_cursor_area` so an unchanged caret (most frames —
    /// caret moves only on key press or text mutation) skips the
    /// `winit` boundary call.
    fn publish_ime_for_window(&mut self, window_id: WindowId, paint_scene: &pinion_core::Scene) {
        let cached_state = *self.core.cached_state();
        let focused_owned = self.core.focus().focused().map(str::to_owned);
        let owner = self.core.root_owner().clone();
        let ime_rect =
            owner.run(|| V::ime_caret_rect(&cached_state, paint_scene, focused_owned.as_deref()));
        let Some(rect) = ime_rect else { return };
        let rect_tuple = (rect.x, rect.y, rect.width, rect.height);
        let Some(slot) = self.windows.get_mut(&window_id) else {
            return;
        };
        if slot.last_ime_cursor_area == Some(rect_tuple) {
            return;
        }
        let RenderState::Active { window, .. } = &slot.render else {
            return;
        };
        let pos = LogicalPosition::new(f64::from(rect.x), f64::from(rect.y));
        let size = LogicalSize::new(
            f64::from(rect.width.max(1.0)),
            f64::from(rect.height.max(1.0)),
        );
        window.set_ime_cursor_area(pos, size);
        slot.last_ime_cursor_area = Some(rect_tuple);
    }

    /// R770 §5.15 — route the three winit file-DnD events to the
    /// binding's `WidgetView` file hooks via [`ShellCore`]. winit
    /// normalises the platform file-DnD (X11 `XdndDrop` / Wayland
    /// data-device / macOS `NSDraggingDestination` / Windows
    /// `IDropTarget`) into window-scoped events (path, no drop
    /// coordinate); the `scene/hover_file` / `scene/hover_file_cancel` /
    /// `scene/drop_file` RPC peers reach the same `ShellCore` methods
    /// (§2 invariant #2). Split out of `window_event` so that dispatch
    /// stays under the line cap; the caller's arm guarantees one of these
    /// three variants, so the wildcard is unreachable in practice.
    fn handle_file_dnd(&mut self, spec_id: &str, event: &WindowEvent) {
        match event {
            WindowEvent::HoveredFile(path) => {
                self.core
                    .file_hover_for_window(spec_id, &path.to_string_lossy());
            }
            WindowEvent::HoveredFileCancelled => {
                self.core.file_hover_cancel_for_window(spec_id);
            }
            WindowEvent::DroppedFile(path) => {
                self.core
                    .file_drop_for_window(spec_id, &path.to_string_lossy());
            }
            _ => {}
        }
    }

    /// R51.78 §5.39 — pressed-key routing surface. Translates the
    /// winit-specific [`Key`] enum into the winit-free
    /// [`ShellCore::handle_focus_traverse`] /
    /// [`ShellCore::handle_character_key`] /
    /// [`ShellCore::handle_named_key`] triple, keeping `Escape`
    /// shell-side because it terminates the event loop.
    ///
    /// Pre-R51.78 this helper did the full dispatch (focus traversal,
    /// `V::keybinding` lookup, `V::apply_key`) inline and was untestable
    /// without a winit `ActiveEventLoop`. R51.78 pushes the substrate
    /// logic into [`ShellCore`] and leaves only the winit↔substrate
    /// adapter shape here.
    fn handle_key_press(&mut self, event_loop: &ActiveEventLoop, logical_key: &Key, repeat: bool) {
        match logical_key.as_ref() {
            // R693 §5.39 — while a modal focus trap is active, Escape
            // dismisses the modal, not the window: route it to the
            // widget's `apply_key` (the dialog binding maps Escape →
            // cancel) instead of `event_loop.exit`. WAI-ARIA modal
            // contract: you cannot Escape past an open dialog to quit;
            // you dismiss the dialog first. With no modal up, Escape
            // keeps the standalone-app convention of closing the window.
            Key::Named(NamedKey::Escape) => {
                // R695 §5.35 — offer Escape to the focused widget first
                // (the Tooltip's WCAG 1.4.13 dismiss, the Dialog's modal
                // cancel). Only fall back to the standalone-app
                // close-window convention when no widget consumes it AND
                // no modal trap is up — you cannot Escape past an open
                // modal to quit (WAI-ARIA modal contract).
                // R1071 PR-27 §5.39 §5.35 — carry the OS auto-repeat flag to
                // the binding (a modal dialog's Escape→cancel is idempotent,
                // but the binding owns the policy uniformly across keys).
                let handled = self.core.try_apply_key_inner("Escape", repeat);
                if !handled && !self.core.focus_is_modal() {
                    event_loop.exit();
                }
            }
            Key::Named(NamedKey::Tab) => {
                // R938 §5.22 §5.39 — offer Tab to the focused widget first (a
                // multi-line code editor with `tab_indents` on indents the
                // selection; Shift+Tab dedents — the shell tracks the Shift
                // bit in `self.core.modifiers`). Only fall back to focus
                // traversal when no widget consumes it — the mirror of the
                // Escape offer-first arm above (a focused Tooltip / Dialog
                // gets first refusal before the shell default). Every
                // non-editor widget reports Tab unhandled, so traversal is
                // byte-unchanged for them.
                if !self.core.try_apply_key_inner("Tab", repeat) {
                    self.core
                        .handle_focus_traverse(self.core.modifiers_shift_key());
                }
            }
            Key::Character(c) => self.core.handle_character_key_inner(c, repeat),
            Key::Named(named) => {
                if let Some(key_str) = named_key_str(named) {
                    self.core.handle_named_key_inner(key_str, repeat);
                }
            }
            _ => {}
        }
    }

    /// R1073 PR-27.4 §5.39 §5.16 §5.35 — body of the
    /// `WindowEvent::KeyboardInput` arm, extracted to keep `window_event`
    /// under the workspace `clippy::too_many_lines` ceiling (the app.rs split
    /// convention shared with [`Self::handle_mouse_button`] /
    /// `handle_file_dnd`). Takes the `Copy` [`WindowId`] (not a borrowed
    /// `spec_id`) and re-resolves the canonical [`WindowSpec::id`](crate::WindowSpec)
    /// internally, so the high-frequency keyboard / auto-repeat path allocates
    /// nothing (the sibling mouse / file arms `to_owned()` because they are
    /// low-frequency).
    fn handle_keyboard_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: &winit::event::KeyEvent,
        is_synthetic: bool,
    ) {
        // R683 §5.16 — re-resolve `WindowId` → canonical spec id (the same
        // fallback `window_event` uses); borrows `self.windows`, disjoint from
        // the `self.core` calls below.
        let spec_id: &str = self
            .windows
            .get(&window_id)
            .map_or(pinion_runtime::DEFAULT_WINDOW, |s| &*s.spec_id);
        let pressed = event.state == ElementState::Pressed;
        // R1073.1 PR-27.4 §5.39 §5.16 §5.35 — resolve TWO vocabularies in one
        // match. `chord_key` (R882 §5.41 `named_key_str`) feeds the chord cache
        // on BOTH edges — auto-repeat re-sends `Pressed`, idempotent against the
        // cache. `gate_key` (`dispatch_named_key_str`, a superset adding the
        // shell-reserved `Escape` / `Tab`) feeds the press-owner gate, so EVERY
        // key `handle_key_press` dispatches is covered, not just chord keys.
        let (chord_key, gate_key): (Option<&str>, Option<&str>) = match event.logical_key.as_ref() {
            Key::Named(named) => (named_key_str(named), dispatch_named_key_str(named)),
            Key::Character(c) => (Some(c), Some(c)),
            _ => (None, None),
        };
        // Passive chord cache tracks PHYSICAL held state on both edges — for
        // synthetic events too, so a key held across a focus transition stays
        // armed (the R882 pan chord's focus continuity). Auto-repeat re-sends
        // `Pressed`, idempotent against the cache.
        if let Some(key_str) = chord_key {
            self.core.note_key_state(key_str, pressed);
        }
        // R1076 PR-28 §5.39 §5.16 §5.35 — gate the key-edge decision through the
        // synthetic-aware ShellCore seam. winit emits SYNTHETIC key events (a
        // `Pressed` for every held key when a window GAINS OS focus, a `Released`
        // when it LOSES focus; `is_synthetic` on X11 / Windows) only to sync key
        // state to a newly-(un)focused window — they are NOT user intent.
        // Dispatching them as real presses self-sustains a dock-toggle flap: a
        // held shortcut's dispatch moves OS focus, the focus change emits a
        // synthetic `Pressed`, which fires the toggle again. `apply_key_edge`
        // excludes synthetic events from the gate, the press-owner lifecycle
        // (R1071 pin / R1073.1 clear), and dispatch; the physical key is unchanged
        // across the transition, so the owner a real keydown pinned survives to
        // the matching physical keyup. For a physical edge it is byte-identical to
        // the pre-R1076 inline gate (R1071 OS-focus + R1073 press-owner snapshot,
        // `gate_key` `None` = a media / dead key the shell does not dispatch).
        // R1078 PR-28.2 — `apply_key_edge` returns `Some(repeat)` for a dispatched
        // edge with the auto-repeat flag DERIVED from the press-owner gate, not
        // winit's `event.repeat`. winit resets its own repeat detector on every
        // focus transition (`x11/event_processor.rs`: the first non-synthetic event
        // after a focus gain is never flagged a repeat), so a held shortcut whose
        // dispatch bounces OS focus would see `event.repeat == false` on each
        // auto-repeat and a repeat-dropping toggle would never drop (the residual
        // sprag dock flap after R1076). The press-owner survives focus transitions
        // by design, so it is the focus-robust repeat source.
        if let Some(repeat) = self
            .core
            .apply_key_edge(spec_id, gate_key, pressed, is_synthetic)
        {
            self.handle_key_press(event_loop, &event.logical_key, repeat);
        }
    }

    /// R51.62 §5.40 — relay one winit `WindowEvent` to the
    /// `accesskit_winit::Adapter` (if attached).
    ///
    /// Every winit event must reach the adapter before the shell
    /// processes it so the AT-side mirror of window bounds / focus
    /// state stays consistent. The adapter handles `Moved` /
    /// `Resized` / `Focused` internally (`set_root_window_bounds` +
    /// `update_window_focus_state`) and ignores everything else, so
    /// the relay is unconditional.
    /// R670.B §5.16 — per-window accesskit relay. Looks up the
    /// `WindowSlot` by `window_id` and forwards the event to that
    /// slot's adapter if attached. AccessKit canonical: 1 adapter
    /// per window (multi-window = multi-adapter, NOT auto-merged) so
    /// the event MUST reach the same window's adapter that the
    /// dispatch is targeting; routing through the primary's adapter
    /// would leak winit coordinates between windows.
    fn forward_to_accesskit(&mut self, window_id: WindowId, event: &WindowEvent) {
        if let Some(slot) = self.windows.get_mut(&window_id)
            && let (Some(adapter), RenderState::Active { window, .. }) =
                (slot.accesskit.as_mut(), &slot.render)
        {
            adapter.process_event(window, event);
        }
    }

    /// Dispatch a winit mouse-button transition to the substrate's
    /// per-window input arcs. Split out of [`Self::window_event`] to keep
    /// that dispatcher under the workspace `clippy::too_many_lines` (100)
    /// ceiling (the app.rs extract convention).
    ///
    /// - **Left Pressed / Released** — pointer down / up
    ///   ([`CoreShell::mouse_pressed_for_window`] /
    ///   [`CoreShell::mouse_released_for_window`]).
    /// - **Middle Pressed / Released** — R881 §5.35 §5.49 middle-button
    ///   gesture pair ([`ShellCore::middle_pressed_for_window`] /
    ///   [`ShellCore::middle_released_for_window`]). The router's
    ///   `DragLatch` resolves the press: a drag past the dead zone pans
    ///   the pinned scrollable / canvas (Blender / Unreal middle-drag),
    ///   a release-in-place runs the R56.2.e `apply_middle_click` paste
    ///   funnel (X11 PRIMARY at the focused text widget — paste moved
    ///   from press to release, the xterm / Qt convention).
    /// - **Right Pressed** — R772 §5.53 `apply_secondary_click`, the
    ///   own-renderer context-menu open path (R771.1: pinion draws its own
    ///   menu on every platform). `secondary_click_for_window` reads the
    ///   cached cursor position for `spec_id` and dispatches through
    ///   [`CoreShell::apply_secondary_click`](pinion_runtime::CoreShell::apply_secondary_click).
    ///
    /// winit normalises each platform's button events (X11 `ButtonEvent` /
    /// Wayland `wl_pointer` button / macOS `NSEvent` / Windows
    /// `WM_*BUTTONDOWN`) under one enum, so these five arms cover every
    /// backend. Other button / state combinations are ignored.
    fn handle_mouse_button(
        &mut self,
        spec_id: &str,
        button: MouseButton,
        state: ElementState,
        event_loop: &ActiveEventLoop,
    ) {
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                // R1121 §5.16 §5.39 — a press on a client-side window-chrome
                // control (borderless title bar) is consumed by the shell, not
                // forwarded to widget routing.
                if self.try_chrome_press(spec_id, event_loop) {
                    return;
                }
                self.core
                    .mouse_pressed_for_window(spec_id, PointerId::MOUSE);
            }
            (MouseButton::Left, ElementState::Released) => {
                self.core
                    .mouse_released_for_window(spec_id, PointerId::MOUSE);
                // R1147 §5.51 — a left release ends the drag session, so hide the
                // cross-desktop drag preview (kept for reuse) + clear suppression.
                self.hide_drag_preview();
            }
            (MouseButton::Middle, ElementState::Pressed) => {
                self.core
                    .middle_pressed_for_window(spec_id, PointerId::MOUSE);
            }
            (MouseButton::Middle, ElementState::Released) => {
                self.core
                    .middle_released_for_window(spec_id, PointerId::MOUSE);
            }
            (MouseButton::Right, ElementState::Pressed) => {
                self.core
                    .secondary_click_for_window(spec_id, PointerId::MOUSE);
            }
            _ => {}
        }
    }

    /// (R1121 §5.16 §5.39) Resolve `spec_id`'s live winit `Window` handle, or
    /// `None` when the window has no active render slot.
    fn window_arc_for_spec(&self, spec_id: &str) -> Option<&Arc<Window>> {
        let window_id = self.spec_id_to_window_id.get(spec_id).copied()?;
        Self::slot_window(self.windows.get(&window_id)?)
    }

    /// (R1121 §5.16 §5.39 §2 #2) Intercept a left-press that landed on a
    /// client-side window-chrome control (a borderless window's title bar).
    /// Returns `true` when the press hit a control and was consumed (the
    /// normal widget routing is then skipped).
    ///
    /// The control is read from the live hover target the `CursorMoved` arm
    /// already recorded, so the SAME hit-test the router uses drives it — an AI
    /// `scene/click` on the introspectable control tag reaches this identical
    /// path (§2 #2). `minimize` / `maximize` / move map straight to the winit
    /// `Window` (pinion owns the handle); `close` routes to `event_loop.exit()`,
    /// the same path `WindowEvent::CloseRequested` (the OS X on a decorated
    /// window) takes. Per-window close (close a secondary, keep the app) needs a
    /// binding close seam and is a follow-up — this matches the current close
    /// model exactly.
    fn try_chrome_press(&mut self, spec_id: &str, event_loop: &ActiveEventLoop) -> bool {
        let Some(action) = self
            .core
            .hover_target_for_window(spec_id, PointerId::MOUSE)
            .and_then(chrome_action_for_tag)
        else {
            return false;
        };
        if matches!(action, ChromeAction::Close) {
            eprintln!(
                "shell: final state = {}",
                V::fmt_state_log(self.core.cached_state()),
            );
            event_loop.exit();
            return true;
        }
        if let Some(window) = self.window_arc_for_spec(spec_id) {
            match action {
                ChromeAction::Minimize => window.set_minimized(true),
                ChromeAction::Maximize => window.set_maximized(!window.is_maximized()),
                ChromeAction::Move => {
                    // OS-driven interactive move; a borderless window has no OS
                    // title bar, so the chrome grip is the move handle.
                    let _ = window.drag_window();
                }
                ChromeAction::Resize(direction) => {
                    // OS-driven interactive resize; a borderless window has no OS
                    // frame, so a chrome resize edge / corner is the grab handle.
                    let _ = window.drag_resize_window(direction);
                }
                ChromeAction::Close => {}
            }
        }
        true
    }

    /// R51.62 §5.40 — dispatch one AT-side event reported by
    /// `accesskit_winit`.
    ///
    /// * `InitialTreeRequested` (AT attaches): trigger a redraw — the
    ///   next `render` will see `Adapter::update_if_active` as active
    ///   and emit the full tree. No state mutation here.
    /// * `ActionRequested(req)`: routed through
    ///   [`ShellCore::handle_action_request`] which lifts the
    ///   AccessKit action into the same dispatch path the winit
    ///   keyboard arm uses.
    /// * `AccessibilityDeactivated`: log only — the adapter stays in
    ///   place so a subsequent AT reconnect can reuse it without
    ///   recreating the per-window state.
    fn handle_accesskit_event(&mut self, event: accesskit_winit::Event) {
        use accesskit_winit::WindowEvent as AccessEvent;
        match event.window_event {
            AccessEvent::InitialTreeRequested => {
                self.core.request_redraw();
            }
            AccessEvent::ActionRequested(req) => {
                self.core.handle_action_request(&req);
            }
            AccessEvent::AccessibilityDeactivated => {
                eprintln!("shell: accesskit deactivated");
            }
        }
    }

    /// R670.B §5.16 — create one window+renderer+accesskit slot for
    /// the given [`WindowSpec`]. Extracted from `resumed()` because
    /// the single-resumed→N-windows fan-out would otherwise blow the
    /// `clippy::too_many_lines` (100) ceiling on the parent fn. Each
    /// spec gets its own winit `Window`, GPU renderer,
    /// `IntrinsicAfterFirstPaint` queue, and
    /// `accesskit_winit::Adapter`.
    ///
    /// `cached_window_id` is the `WindowId` from a prior suspend
    /// cycle if any (mobile platform suspend / resume reuse path).
    /// `make_primary` is `true` for the very first spec only — that
    /// spec's `WindowId` is recorded as `primary_window_id` so RPC
    /// frames omitting `{window: "..."}` default to it (R670.B
    /// atomic 1 wire).
    /// R683 §5.16 §5.41 — install the reconcile [`Effect`] that
    /// subscribes the binding's
    /// [`WidgetView::windows_signal`] `Signal<Vec<WindowSpec>>` and
    /// wakes the shell via [`AppEvent::WindowsDirty`] on every
    /// value-changing emit.
    ///
    /// Idempotent: never installs a second Effect on the same
    /// [`AppShell`] (gated by `self.reconcile_effect.is_some()` at
    /// the call site in [`Self::resumed`]). The shell only installs
    /// the
    /// Effect the very first time `resumed()` sees a non-`None`
    /// `windows_signal()`; subsequent suspend / resume cycles reuse
    /// the existing Effect because the same signal handle is
    /// memoised on the binding side via [`Owner::cache`].
    ///
    /// The Effect closure captures three values by move:
    ///
    /// 1. `Rc<Signal<Vec<WindowSpec>>>` — `.get()` on every rerun
    ///    establishes the dependency on the signal so future
    ///    mutations fire `rerun` again. The closure does NOT use
    ///    the snapshot — diff happens in
    ///    [`Self::reconcile_windows`] where `&mut self` +
    ///    [`ActiveEventLoop`](winit::event_loop::ActiveEventLoop)
    ///    are available.
    /// 2. `EventLoopProxy<AppEvent>` — sends
    ///    [`AppEvent::WindowsDirty`] to wake the shell from any
    ///    blocking wait state. The proxy clone is `'static` and
    ///    survives the closure's lifetime (winit guarantees the
    ///    proxy is valid as long as the event loop is running).
    /// 3. `Rc<RefCell<Vec<WindowSpec>>>` — not currently captured;
    ///    reserved for a follow-up where the closure pre-computes
    ///    add/drop diffs to avoid wake / re-read costs on identical
    ///    re-emits. v1 keeps the closure minimal so the wake-up cost
    ///    is just `signal.get()` + `proxy.send_event(..)`.
    ///
    /// `send_event` failure (the event loop already exited) is
    /// silently ignored — the Effect closure has no recovery path
    /// at that point.
    ///
    /// The Effect's eager initial run fires `WindowsDirty` once
    /// immediately; the subsequent `reconcile_windows` call no-ops
    /// because [`Self::last_known_specs`] is initialised to the
    /// same snapshot the Effect just observed.
    fn install_reconcile_effect(&mut self, signal: Rc<Signal<Vec<WindowSpec>>>) {
        let signal_for_closure = Rc::clone(&signal);
        let proxy = self.proxy.clone();
        let effect = self.core.root_owner().run(|| {
            let owner = pinion_core::Owner::current()
                .expect("install_reconcile_effect runs inside root_owner.run wrap");
            Effect::new(&owner, move || {
                // Subscribe by reading the signal value — the Effect
                // re-runs whenever a `Signal::set` actually changes
                // the inner `Vec<WindowSpec>` (R26 equality-skip).
                let _ = signal_for_closure.get();
                // Wake the shell so the user_event handler can call
                // `reconcile_windows` with `&mut self` + the active
                // event loop. Failure means winit already exited;
                // the closure has no recovery path at that point.
                let _ = proxy.send_event(AppEvent::WindowsDirty);
            })
        });
        self.windows_signal = Some(signal);
        self.reconcile_effect = Some(effect);
    }

    /// R683 §5.16 §5.41 — diff the freshly-emitted
    /// `Signal<Vec<WindowSpec>>` snapshot against
    /// [`Self::last_known_specs`] and reconcile.
    ///
    /// **Add pass**: every spec id in the new snapshot that is not
    /// in the last-known cache resumes via the existing
    /// [`Self::resume_spec`] helper (one winit `Window` + GPU
    /// renderer + `accesskit_winit::Adapter` per spec, plus the
    /// per-window `WindowSlot` cluster + `spec_id_to_window_id`
    /// reverse-map entry).
    ///
    /// **Drop pass**: every spec id in the last-known cache that is
    /// not in the new snapshot drops the matching `WindowSlot` (the
    /// `RenderState::Active`'s `Arc<Window>` releases; winit closes
    /// the OS window when the last ref drops). The shell-side
    /// [`crate::ShellCore::remove_window`] call drains every
    /// per-window substrate state map
    /// (`redraw_requested_per_window` / `last_paint_instants` /
    /// `target_fps_per_window` / `fragment_cache_stats_per_window`),
    /// then forwards into the runtime-side
    /// [`pinion_runtime::CoreShell::remove_window`] which drops the
    /// `routers` + `window_owners` entries (the per-window `Owner`
    /// drop fires the cleanup queue for every animation / command /
    /// cache slot registered on that scope).
    ///
    /// **Idempotency**: returns immediately when
    /// `new_specs == old_specs` (`Vec` element-wise `PartialEq`); the
    /// Effect's eager initial run lands here on the very first
    /// install and the snapshot equality short-circuit keeps the
    /// no-op path cheap.
    ///
    /// **Primary protection**: the canonical primary spec
    /// (`WindowSpec::main`, id `"main"`) is the binding's reactive
    /// substrate anchor; the shell-side substrate refuses to remove
    /// it (`ShellCore::remove_window` returns `false` for
    /// `DEFAULT_WINDOW`). A binding that drops `"main"` from its
    /// `windows_signal()` list will see the AppShell-side
    /// `WindowSlot` drop but the substrate state survives —
    /// canonical behaviour for the dock + tear-off arc (the main
    /// dock surface stays alive as the "host" for every torn-off
    /// panel).
    fn reconcile_windows(&mut self, event_loop: &ActiveEventLoop) {
        let Some(signal) = self.windows_signal.as_ref() else {
            // No opt-in, no reconcile — defensive against a
            // WindowsDirty arriving before install (degenerate corner
            // case).
            return;
        };
        let new_specs = signal.get();
        let old_specs: Vec<WindowSpec> = self.last_known_specs.borrow().clone();
        if new_specs == old_specs {
            // Idempotent fast-path — identical re-emit, no add / drop
            // work to do. The Effect's eager initial run lands here
            // because `install_reconcile_effect` seeds
            // `last_known_specs` from the same snapshot.
            return;
        }
        // Build the id sets once so the difference walks below are
        // O(N) instead of O(N²). `HashSet<&str>` so the lookups are
        // alloc-free (Cow derefs to str through the iterator).
        let new_ids: HashSet<&str> = new_specs.iter().map(|s| s.id.as_ref()).collect();
        let old_ids: HashSet<&str> = old_specs.iter().map(|s| s.id.as_ref()).collect();
        // Drop pass: in old, not in new. Materialise the to-drop
        // list as owned `String`s so the subsequent
        // `&mut self.windows` mutation does not conflict with the
        // `&self.last_known_specs` borrow chain above.
        let to_drop: Vec<String> = old_ids
            .difference(&new_ids)
            .map(|s| (*s).to_owned())
            .collect();
        for spec_id in to_drop {
            // Look up + remove the spec_id → WindowId reverse-map
            // entry. The Cow<str> key resolves through `&str` via
            // `Borrow<str>` for an alloc-free lookup.
            if let Some(window_id) = self.spec_id_to_window_id.remove(spec_id.as_str()) {
                // Drop the WindowSlot — the OS window closes when
                // the last `Arc<Window>` ref drops (the slot's
                // RenderState::Active::window + the
                // accesskit_winit::Adapter both held strong refs;
                // dropping the slot releases the AppShell-side ref,
                // and the adapter drops with the slot since it's a
                // field of the slot).
                self.windows.remove(&window_id);
                if self.primary_window_id == Some(window_id) {
                    self.primary_window_id = None;
                }
                // Drain the per-window substrate state.
                // `ShellCore::remove_window` refuses DEFAULT_WINDOW
                // — the primary scope stays alive even if the
                // binding drops `"main"` from the signal (the
                // primary's reactive substrate is the
                // binding-wide anchor and tearing it down would
                // orphan every `Owner::cache` slot on root_owner).
                let _ = self.core.remove_window(&spec_id);
                eprintln!("shell: closed window {spec_id}");
            }
        }
        // Add pass: in new, not in old. `resume_spec` creates one
        // winit Window + GPU renderer + accesskit_winit::Adapter
        // per spec and inserts the matching `WindowSlot` +
        // `spec_id_to_window_id` entry.
        for spec in &new_specs {
            if !old_ids.contains(spec.id.as_ref()) {
                // `make_primary == false` — the primary was assigned
                // during the initial `resumed()` and survives
                // reconcile passes; runtime-added windows are always
                // secondary.
                self.resume_spec(event_loop, spec, None, false);
            }
        }
        // R1087 §5.16 PR-31 — move pass: a spec present in BOTH old and
        // new whose declared position changed drives the live OS window to
        // the new logical-pixel position. `window_position_moves` is a TOTAL
        // diff (every same-id position change appears; without it the
        // id-keyed add/drop passes would silently swallow it). The apply
        // here is best-effort: a window with no live arc — a
        // `Suspended(Some)` mobile state, `slot_window` → `None` — is skipped
        // and reconciles on its next create (mobile-lifecycle, deferred with
        // mobile). The silent skip on a missing `spec_id_to_window_id` entry
        // mirrors the drop pass's identical pattern (absence ⇒ creation
        // already failed + the loop is exiting). The drag-follow PR-31 builds
        // on top writes the position signal each pointer move; this is where
        // each write lands on the real window. (The live move is HW-gated;
        // the diff is unit-tested in `r1087_window_position_move_diff_tests`.)
        for (spec_id, (x, y)) in window_position_moves(&old_specs, &new_specs) {
            if let Some(window_id) = self.spec_id_to_window_id.get(spec_id.as_str()).copied() {
                if let Some(slot) = self.windows.get_mut(&window_id) {
                    // Clone the arc first so the immutable `slot_window`
                    // borrow ends before the `last_commanded_position`
                    // mutation below.
                    if let Some(window) = Self::slot_window(slot).cloned() {
                        window.set_outer_position(LogicalPosition::new(f64::from(x), f64::from(y)));
                        // R1088 §5.16 PR-31 — latch the commanded position
                        // so the OS `Moved` echo this `set_outer_position`
                        // triggers is recognised + suppressed by
                        // `note_window_moved`, not mistaken for a user drag.
                        slot.last_commanded_position = Some((x, y));
                    }
                }
            }
        }
        // R1118 §5.16 PR-38 — `decorations` is create-time-only (like
        // `strategy`): a same-id runtime flip is NOT applied (no
        // `Window::set_decorations` call exists). Make that trap LOUD rather than
        // a silent no-op, so a future consumer that toggles a live window's
        // chrome gets a signal instead of nothing (fail clearly). Only POSITION
        // closes the OS-feedback loop; chrome + size are read once at create.
        for spec in &new_specs {
            if let Some(old) = old_specs.iter().find(|o| o.id == spec.id) {
                if old.decorations != spec.decorations {
                    eprintln!(
                        "shell: window {} decorations change ({} -> {}) ignored — \
                         decorations is create-time-only; recreate the window to change chrome",
                        spec.id, old.decorations, spec.decorations,
                    );
                }
            }
        }
        // Update the cache so the next `reconcile_windows` call
        // diffs against the snapshot the shell just acted on.
        *self.last_known_specs.borrow_mut() = new_specs;
        // Re-request paint on every active window so the next event
        // loop iteration renders the new topology. drain dispatches
        // a Window::request_redraw per active slot.
        self.core.request_redraw();
        self.drain_redraw_to_winit();
    }

    #[allow(
        clippy::too_many_lines,
        reason = "cohesive single-window creation routine — winit Window + GPU renderer + AccessKit adapter + slot assembly are one transactional unit; the R1088 Suspended-resume position re-apply belongs inline beside the create-time with_position it mirrors"
    )]
    fn resume_spec(
        &mut self,
        event_loop: &ActiveEventLoop,
        spec: &WindowSpec,
        cached_window_id: Option<WindowId>,
        make_primary: bool,
    ) {
        // Pull the cached window arc if any (suspend-resume reuse).
        let cached_window = cached_window_id
            .and_then(|id| self.windows.remove(&id))
            .and_then(|slot| match slot.render {
                RenderState::Active { window, .. } | RenderState::Suspended(Some(window)) => {
                    Some(window)
                }
                RenderState::Suspended(None) => None,
            });
        // R668 §5.16 — read the spec's window-creation policy.
        // `Fixed` opens at exactly `(width, height)`;
        // `IntrinsicAfterFirstPaint` opens at `min` and queues a
        // post-first-paint resize request via
        // `pending_intrinsic_resize`. Either way the renderer
        // initialises against `window.inner_size()`.
        let strategy = spec.strategy;
        let (init_w, init_h) = strategy.initial_logical_size();
        let window = if let Some(w) = cached_window {
            // R1088 §5.16 §5.41 PR-31 — re-apply the declared position on a
            // `Suspended(Some)` resume. `with_position` only applies at
            // CREATE (the `else` branch); a cached window reused across a
            // suspend/resume cycle would otherwise keep its pre-suspend OS
            // position, drifting the live window from the declared
            // `windows_signal` (R1087.1 finding ②). This closes the
            // move-pass apply gap for the mobile-lifecycle resume path. The
            // matching `last_commanded_position` latch is stamped on the
            // rebuilt slot below (suppresses the resulting `Moved` echo).
            if let Some((x, y)) = spec.position {
                w.set_outer_position(LogicalPosition::new(f64::from(x), f64::from(y)));
            }
            w
        } else {
            let mut attrs = Window::default_attributes()
                .with_title(spec.title.clone())
                .with_inner_size(LogicalSize::new(f64::from(init_w), f64::from(init_h)));
            // R1087 §5.16 PR-31 — honour the declared logical-pixel outer
            // position when the binding pins one (the floating dock-panel
            // tear-off opens its window under the cursor at detach).
            // `None` — every pre-R1087 spec — leaves placement to the
            // window manager exactly as before (byte-identical). winit
            // applies the per-monitor DPI scale to the logical coords.
            if let Some((x, y)) = spec.position {
                attrs = attrs.with_position(LogicalPosition::new(f64::from(x), f64::from(y)));
            }
            // R1115 §5.16 §5.51 PR-38 — honour the declared OS chrome. A
            // torn-off dock panel declares `decorations: false` so the OS
            // draws no title bar/border and pinion owns the panel's chrome
            // (its own header + drag-grip). winit's default is `true`, so
            // setting it to the spec value (`true` for every pre-R1115
            // binding) is byte-identical. Create-time only — like
            // `strategy`, not re-applied on a same-id spec change (a
            // dock-back destroys the floating window, never re-decorates).
            attrs = attrs.with_decorations(spec.decorations);
            // R835 §5.16 — windowless test mode. `PINION_HIDDEN_WINDOW`
            // creates the shell window UNMAPPED (`visible = false`): Vello
            // still renders to the GPU surface and `scene/snapshot` /
            // `scene/query` work unchanged, but no window flashes on the
            // developer's real display. Headless RPC demos set this so a
            // local verification run does not seize focus / flicker.
            // Unset (the default) keeps the window visible for interactive
            // `run` / `verify` sessions.
            if hidden_window_requested() {
                attrs = attrs.with_visible(false);
            }
            // R668 §5.16 / R1059 — anchor the user-driven OS-resize
            // floor via the single-source-of-truth policy on
            // `SizeStrategy`. `Fixed`/`IntrinsicAfterFirstPaint` pin
            // the floor at their open size / `min`; `OpenResizable`
            // forwards its independent `min`, and `None` skips
            // `with_min_inner_size` entirely so the window stays at the
            // OS-native minimum (~100×30 desktop) — the freely
            // shrinkable case.
            if let Some(min_floor) = strategy.min_inner_floor() {
                attrs = attrs.with_min_inner_size(LogicalSize::new(
                    f64::from(min_floor.0),
                    f64::from(min_floor.1),
                ));
            }
            match event_loop.create_window(attrs) {
                Ok(w) => Arc::new(w),
                Err(e) => {
                    eprintln!("shell: window create ({}) failed: {e}", &spec.id);
                    event_loop.exit();
                    return;
                }
            }
        };
        // R889 §5.16 §5.49 — register the window in the substrate's
        // window-known registry the moment the OS window exists
        // (before renderer init + before the first paint). From here
        // on `CoreShell::is_window_known(spec.id)` is true, so the
        // dispatch-entry unknown-window gate admits RPC scoped to this
        // window and the per-window READ axes (`scene/input_state` /
        // `scene/pacing_state`) answer honestly for the
        // registered-but-unpainted phase (R683 tear-off arc). One
        // creation edge for both `resumed` and `reconcile_windows`
        // paths; the matching removal edge is the reconcile drop
        // pass's `ShellCore::remove_window`.
        self.core.register_window(spec.id.as_ref());
        let pending_intrinsic_resize = match strategy {
            // R1059 — `OpenResizable` opens at `size` with no
            // post-first-paint resize, exactly like `Fixed`; only
            // `IntrinsicAfterFirstPaint` queues the measure-and-resize.
            SizeStrategy::Fixed { .. } | SizeStrategy::OpenResizable { .. } => None,
            SizeStrategy::IntrinsicAfterFirstPaint { min, max } => Some((min, max)),
        };
        // R56.2.a §5.13 §5.38 — opt into the winit IME bridge per
        // window so `WindowEvent::Ime` events flow into
        // `Self::window_event` and reach `ShellCore::apply_composition`
        // → `WidgetCore::apply_composition`. Per-window because IME
        // sessions are window-scoped on every platform.
        window.set_ime_allowed(true);
        // R57.1 §5.50 — push the initial OS `prefers-color-scheme`
        // reading into the global signal (only needed on the primary
        // window; the signal is process-global). Secondary window
        // re-pushes are idempotent so the gate is omitted for
        // simplicity.
        if let Some(theme) = window.theme() {
            pinion_core::set_system_color_scheme(winit_theme_to_pinion_scheme(theme));
        }
        // R1027 §5.16 — seed the per-slot scale factor from the OS so the
        // first paint already lays out logical and rasters at the device
        // resolution (refreshed later by `WindowEvent::ScaleFactorChanged`).
        // The renderer surface is sized in physical pixels (unchanged).
        let scale_factor = window.scale_factor();
        // R1147 §5.16 — renderer init via the shared `build_renderer` helper
        // (also used by the drag-preview window), so both window paths cross the
        // §6.3 `pollster::block_on` boundary the same way.
        let renderer = match Self::build_renderer(&window) {
            Ok(r) => *r,
            Err(e) => {
                eprintln!("shell: renderer init ({}) failed: {e}", &spec.id);
                // Cache the window for a subsequent retry; renderer
                // init failed but the OS window survives.
                let window_id = window.id();
                // R683 §5.16 — `spec.id` is `Cow<'static, str>` so
                // `.clone()` produces a fresh owned handle for the
                // per-slot copy + the spec_id_to_window_id map key.
                // `Cow::Borrowed` clones are pointer-cheap; runtime
                // ids (`Cow::Owned`) pay one `String::clone`.
                self.windows.insert(
                    window_id,
                    WindowSlot::build(
                        RenderState::Suspended(Some(window)),
                        None,
                        spec.id.clone(),
                        pending_intrinsic_resize,
                        scale_factor,
                    ),
                );
                self.spec_id_to_window_id.insert(spec.id.clone(), window_id);
                event_loop.exit();
                return;
            }
        };
        // R51.62 §5.40 — construct the per-window accesskit_winit
        // Adapter. AccessKit canonical: 1 adapter = 1 window. The
        // proxy is cloned because Adapter consumes one internally
        // for each of its three handler hooks (activation / action /
        // deactivation).
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        let window_id = window.id();
        let mut slot = WindowSlot::build(
            RenderState::Active {
                window,
                renderer: Box::new(renderer),
            },
            Some(adapter),
            spec.id.clone(),
            pending_intrinsic_resize,
            scale_factor,
        );
        // R1088 §5.16 §5.41 §2 #7 PR-31 — latch the declared position the
        // create (`with_position`) or `Suspended`-resume re-apply just
        // commanded, so the window's first `WindowEvent::Moved` (the OS
        // echo of that placement) is suppressed by `note_window_moved`
        // rather than written back as if it were a user drag. `None` for a
        // WM-placed window leaves the latch empty.
        slot.last_commanded_position = spec.position;
        self.windows.insert(window_id, slot);
        self.spec_id_to_window_id.insert(spec.id.clone(), window_id);
        if make_primary {
            self.primary_window_id = Some(window_id);
        }
        eprintln!(
            "shell: {} resumed (window {}; initial size {}x{})",
            spec.title, &spec.id, init_w, init_h,
        );
    }

    /// R670.B / R1023 / R1123 §5.16 §5.39 — per-window resize. Forwards the
    /// surface resize to the live GPU renderer, repaints THIS window (winit
    /// does not guarantee a `RedrawRequested` after a `Resized`, so without
    /// this the reconfigured swapchain presents a stale backbuffer until some
    /// unrelated redraw arrives — the live drag-resize ghosting on full-bleed
    /// content), and syncs the per-window maximized cache from winit's actual
    /// `Window::is_maximized()`. The cache feeds the client-side chrome glyph
    /// (maximize vs restore) and the resize-border suppression on both the live
    /// paint and the pure mirror, so `scene/snapshot` matches the painted glyph
    /// (§2 #7). Reading winit's report (not just the chrome-button intent)
    /// keeps the cache correct when a tiling WM maximizes the window.
    fn note_window_resized(&mut self, window_id: WindowId, size: PhysicalSize<u32>) {
        let spec_id = self
            .windows
            .get(&window_id)
            .map_or(pinion_runtime::DEFAULT_WINDOW, |s| &*s.spec_id)
            .to_owned();
        let mut maximized = None;
        if let Some(slot) = self.windows.get_mut(&window_id)
            && let RenderState::Active {
                renderer, window, ..
            } = &mut slot.render
        {
            renderer.resize(size.width.max(1), size.height.max(1));
            maximized = Some(window.is_maximized());
            // winit coalesces repeated `request_redraw` before the next
            // `RedrawRequested`, so a fast drag costs at most one paint/frame.
            window.request_redraw();
        }
        if let Some(m) = maximized {
            self.core.set_maximized_for_window(&spec_id, m);
        }
    }

    /// R1027 §5.16 — the window moved to a display with a different DPI,
    /// or the OS scale changed. Refresh the cached factor so the next paint
    /// lays out logical -> rasters at the new device resolution and pointer
    /// events convert against the new scale. winit pairs this event with a
    /// `Resized` (the new physical inner size) that reconfigures the GPU
    /// surface; the explicit redraw here makes the rescaled frame appear
    /// immediately rather than waiting on an unrelated redraw.
    /// `inner_size_writer` is left untouched — pinion accepts winit's
    /// recommended physical size.
    fn note_scale_factor_changed(&mut self, window_id: WindowId, scale_factor: f64) {
        if let Some(slot) = self.windows.get_mut(&window_id) {
            slot.scale_factor = scale_factor;
            if let RenderState::Active { window, .. } = &slot.render {
                window.request_redraw();
            }
        }
    }

    /// R1088 §5.16 §5.41 §2 #7 PR-31 — feed a winit `WindowEvent::Moved`
    /// back into `windows_signal` so the DECLARED position converges on the
    /// actual one (the architecture-A ideal: a user native title-bar drag
    /// writes the position SSOT, and `scene/windows` then reads the live
    /// placement, declared == actual).
    ///
    /// **Echo suppression.** The reconcile move pass and the
    /// `Suspended`-resume re-apply record every position they command in
    /// [`WindowSlot::last_commanded_position`]. A `Moved` equal to that
    /// latch is the shell's OWN `set_outer_position` echoing back — it is
    /// consumed (the latch clears) and NOT written, so a shell- or
    /// RPC-driven move does not loop (`command -> Moved -> signal write ->
    /// reconcile -> command -> ...`). A `Moved` that DIVERGES is a user /
    /// WM drag and is written back.
    ///
    /// **Conservative scope.** Only a window that ALREADY declares a
    /// position (the floating tear-off panels) is updated; a `None`
    /// WM-placed window (the typical `"main"`) is left WM-managed — one
    /// user drag must not silently convert it into a pinned window.
    ///
    /// The write syncs [`Self::last_known_specs`] to the same snapshot
    /// before emitting, so the signal write fires the reconcile effect but
    /// hits its `new == old` fast path: the OS window is NOT re-commanded
    /// back to where the user just put it.
    fn note_window_moved(&mut self, window_id: WindowId, position: PhysicalPosition<i32>) {
        let Some(slot) = self.windows.get(&window_id) else {
            return;
        };
        let scale = slot.scale_factor;
        let spec_id = slot.spec_id.clone();
        let commanded = slot.last_commanded_position;
        // winit reports the outer position in PHYSICAL px; the signal +
        // `scene/windows` are LOGICAL px (§5.21). `to_logical::<i32>`
        // divides by the per-window scale and rounds (winit's i32 `Pixel`),
        // matching the `set_outer_position(LogicalPosition)` the move pass
        // issues (winit multiplies logical -> physical on the way out).
        let logical: LogicalPosition<i32> = position.to_logical(scale);
        let logical = (logical.x, logical.y);
        // Echo of our own command? Consume the latch (clear it) without
        // writing. `moved_is_command_echo` is the pure, unit-tested core.
        if moved_is_command_echo(commanded, logical) {
            if let Some(slot) = self.windows.get_mut(&window_id) {
                slot.last_commanded_position = None;
            }
            return;
        }
        // A user / WM move. Compute the write-back via the pure, unit-tested
        // `user_move_writeback` (the conservative-scope filter + idempotency
        // skip + spec lookup live there); `None` = nothing to write.
        let Some(signal) = self.windows_signal.as_ref() else {
            return;
        };
        let Some(new_specs) = user_move_writeback(signal.get(), spec_id.as_ref(), logical) else {
            return;
        };
        // Sync the reconcile cache to the snapshot we are about to emit, so
        // the signal write does NOT re-command the OS window to where the
        // user just dragged it (the move pass would otherwise emit a
        // redundant `set_outer_position`). The effect still fires + lands
        // on the `new == old` fast path.
        (*self.last_known_specs.borrow_mut()).clone_from(&new_specs);
        signal.set(new_specs);
    }
}

impl<V: WidgetView> ApplicationHandler<AppEvent> for AppShell<V> {
    /// R46.3.4 — winit may fire `resumed` more than once on platforms
    /// that suspend (Android, Wayland-compositor focus changes). The
    /// Vello canonical pattern caches the previous `Window` across
    /// the drop-and-recreate cycle so the OS-side handle survives,
    /// while the GPU renderer is freshly constructed each time.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // R670.B §5.16 — single-window legacy behaviour was "skip if
        // already Active". Multi-window equivalent: skip if every
        // declared spec has already been created. We resume per-spec
        // (suspended slots keep their cached `Window` Arc) so the
        // mobile suspend-resume lifecycle still works per slot.
        //
        // R683 §5.16 §5.41 — read the binding's optional
        // [`WidgetView::windows_signal`] first. When `Some(signal)`,
        // the signal's snapshot replaces `V::windows()` as the
        // initial spec list AND the reconcile Effect will subscribe
        // to subsequent mutations (dock tear-off / dock-back). When
        // `None` (the pre-R683 default for every single + multi-
        // window binding) the compile-time `V::windows()` list is
        // the source of truth + window topology is frozen for the
        // binding's lifetime — bit-identical to the pre-R683
        // contract.
        //
        // The trait call is wrapped in `root_owner.run(..)` so the
        // binding impl can reach `Owner::current()` + use
        // `Owner::cache` to memoise the returned signal across
        // shell-side re-entries (suspend / resume cycles read the
        // same memoised `Rc<Signal<..>>` so the Effect install gate
        // below sees the same handle).
        let opt_signal = self.core.root_owner().run(V::windows_signal);
        let specs: Vec<WindowSpec> = match opt_signal.as_ref() {
            Some(signal) => signal.get(),
            None => V::windows(),
        };
        if specs.is_empty() {
            eprintln!("shell: V::windows() returned empty list; nothing to create",);
            event_loop.exit();
            return;
        }
        let mut primary_assigned = false;
        for spec in &specs {
            // Resolve any cached window from a prior suspend cycle
            // (look up by spec id). The first `resumed()` after boot
            // never has cached entries; subsequent post-suspended
            // resumes can re-attach the cached `Window` to a fresh
            // GPU renderer.
            //
            // R683 §5.16 — `spec.id` is `Cow<'static, str>`; the
            // HashMap `.get(K: Borrow<Q>)` resolves through Cow's
            // `Borrow<str>` impl, so the `.get(&*spec.id)` form
            // passes a plain `&str` and avoids cloning the Cow.
            let cached_window_id = self.spec_id_to_window_id.get(&*spec.id).copied();
            // Skip specs that already have an Active slot. A spec is
            // either fully Active or fully Suspended (no mid-state).
            if let Some(window_id) = cached_window_id
                && let Some(slot) = self.windows.get(&window_id)
                && matches!(slot.render, RenderState::Active { .. })
            {
                if !primary_assigned {
                    self.primary_window_id = Some(window_id);
                    primary_assigned = true;
                }
                continue;
            }
            self.resume_spec(event_loop, spec, cached_window_id, !primary_assigned);
            if !primary_assigned && self.spec_id_to_window_id.contains_key(&*spec.id) {
                primary_assigned = true;
            }
        }
        // R47.7.5 — winit does not auto-emit `RedrawRequested` on
        // `resumed` (platform-dependent). Explicitly request the
        // first redraw so every active window's first paint commits
        // before the first AI client `scene/layout {viewport: null}`
        // lands. drain_redraw_to_winit walks all windows.
        self.core.request_redraw();
        self.drain_redraw_to_winit();
        // R683 §5.16 §5.41 — install the reconcile Effect after the
        // initial spec resume completes. Gated by
        // `self.reconcile_effect.is_none()` so subsequent
        // `resumed()` calls (mobile suspend / resume cycles) do not
        // install a second Effect on top of the existing
        // subscription.
        if let Some(signal) = opt_signal
            && self.reconcile_effect.is_none()
        {
            // Seed `last_known_specs` to the same snapshot the
            // Effect's eager initial run will observe — the diff
            // short-circuits to a no-op on the first
            // [`AppEvent::WindowsDirty`] arrival.
            *self.last_known_specs.borrow_mut() = specs;
            self.install_reconcile_effect(signal);
        }
    }

    /// R46.3.4 — release the GPU-side renderer on suspend so the OS
    /// can reclaim the wgpu surface. The winit window itself is
    /// cached for the next `resumed` so its handle / OS state
    /// survives.
    ///
    /// R670.B §5.16 — per-window suspend. Walks every `WindowSlot` +
    /// drops its GPU renderer + keeps the `Window` Arc cached. The
    /// `accesskit_winit::Adapter` stays attached so AT-side state
    /// survives the suspend (the adapter only forwards events; the
    /// GPU surface is what mobile platforms reclaim).
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        for slot in self.windows.values_mut() {
            if let RenderState::Active { window, .. } =
                core::mem::replace(&mut slot.render, RenderState::Suspended(None))
            {
                slot.render = RenderState::Suspended(Some(window));
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // R670.B §5.16 — route the AccessKit relay to the matching
        // window's adapter. AccessKit canonical: 1 adapter = 1
        // window; forwarding to the wrong adapter would leak winit
        // event coordinates between windows.
        self.forward_to_accesskit(window_id, &event);
        // R672 §5.35 §5.41 — resolve winit WindowId to canonical
        // [`crate::WindowSpec::id`] before dispatching pointer events
        // so each window's [`pinion_runtime::InputRouter`] handles
        // its own cursor + hover state. Slot lookup is O(1) on the
        // HashMap; spec_id falls back to
        // [`pinion_runtime::DEFAULT_WINDOW`] when the WindowId is not
        // tracked (a Resumed event that has not landed yet — winit
        // can emit some events between AppShell::resumed creating the
        // window and the slot being inserted into the map).
        // R683 §5.16 — `s.spec_id` is `Cow<'static, str>`; `&*s.spec_id`
        // re-borrows as `&str` so the downstream substrate signatures
        // (which take `&str`) stay unchanged. The `&'static str`
        // fallback (`DEFAULT_WINDOW`) coerces to `&str` trivially.
        let spec_id: &str = self
            .windows
            .get(&window_id)
            .map_or(pinion_runtime::DEFAULT_WINDOW, |s| &*s.spec_id);
        // R1027 §5.16 §5.35 — the addressed window's scale factor, copied
        // out (so it does not extend the `self.windows` borrow into the
        // arms). Used to map physical pointer coordinates -> logical for
        // `CursorMoved` / `Touch`. `1.0` for an untracked window (a
        // Resumed event landing before the slot is inserted), which is the
        // pre-R1027 behaviour.
        let scale = self.windows.get(&window_id).map_or(1.0, |s| s.scale_factor);
        match event {
            WindowEvent::CloseRequested => {
                eprintln!(
                    "shell: final state = {}",
                    V::fmt_state_log(self.core.cached_state()),
                );
                event_loop.exit();
            }
            // R48 / R51.80 §5.35: all pointer routing flows through
            // the framework `InputRouter` via [`ShellCore`] wrapper
            // methods. The handler arms only translate winit events
            // into the substrate's pinion-native shape.
            WindowEvent::CursorMoved { position, .. } => {
                // R1148/R1147 §5.51 — body lives in `handle_cursor_moved` (stamp
                // live window origins for cross-window redock + forward the
                // logical cursor + drive the cross-desktop drag preview).
                // Delegating keeps `window_event` under the line cap.
                self.handle_cursor_moved(window_id, position, scale, event_loop);
            }
            WindowEvent::CursorLeft { .. } => {
                self.core.cursor_left_for_window(spec_id, PointerId::MOUSE);
            }
            // R770 §5.15 — OS file drag-drop (winit normalises the
            // platform file-DnD into three window-scoped events). One
            // delegating arm keeps `window_event` under the line cap; the
            // body lives in `handle_file_dnd`.
            ev @ (WindowEvent::HoveredFile(_)
            | WindowEvent::HoveredFileCancelled
            | WindowEvent::DroppedFile(_)) => {
                // `spec_id` borrows `self.windows`; the `&mut self`
                // helper needs an owned id (file events are rare).
                let sid = spec_id.to_owned();
                self.handle_file_dnd(&sid, &ev);
            }
            // R56.2.e / R772 §5.13 §5.22 §5.53 — Left / Middle / Right
            // mouse-button presses + the Left release. Extracted to
            // `handle_mouse_button` to keep this dispatcher under the
            // workspace `clippy::too_many_lines` (100) ceiling (the
            // app.rs split convention). `spec_id` borrows `self.windows`,
            // so the `&mut self` helper needs an owned id (the file-event
            // arc above does the same `to_owned()`).
            WindowEvent::MouseInput { state, button, .. } => {
                let sid = spec_id.to_owned();
                self.handle_mouse_button(&sid, button, state, event_loop);
            }
            // (R51.186 §5.45 R55.C.2) winit `MouseWheel` events do
            // not carry a position field — winit follows the same
            // W3C / iOS / Android contract pinion's router reads:
            // the wheel applies to the surface under the last
            // cursor position. The substrate's `InputRouter`
            // remembers that position, so this arm only needs to
            // convert the unit-tagged `MouseScrollDelta` into the
            // matching pinion-native [`WheelDelta`] and forward.
            WindowEvent::MouseWheel { delta, .. } => {
                // R1027 §5.16 §5.45 — `scale` converts a `PixelDelta`
                // (physical) to logical, mirroring the `CursorMoved` arm.
                let pinion_delta = winit_wheel_to_pinion(delta, scale);
                self.core
                    .wheel_for_window(spec_id, PointerId::MOUSE, pinion_delta);
            }
            // R51.45 §5.35 — winit `WindowEvent::Touch` closes the
            // R51.38 multi-pointer first-design substrate arc.
            // R51.108 §5.41 — convert at the winit boundary so the
            // substrate sees only the abstract `pinion_runtime::Touch`.
            WindowEvent::Touch(touch) => {
                // R1027 §5.16 §5.35 — pass `scale` so the touch's physical
                // location maps to logical (mirrors `CursorMoved`).
                self.core
                    .touch_event_for_window(spec_id, winit_touch_to_pinion(touch, scale));
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // R51.53 §5.39 — winit emits `KeyEvent` without
                // modifier state, so cache the most-recent value
                // out-of-band for Shift+Tab detection.
                // R51.108 §5.41 — convert at the winit boundary.
                self.core
                    .set_modifiers(winit_modifiers_to_pinion(modifiers.state()));
            }
            WindowEvent::Focused(focused) => {
                // R1071 PR-27 §5.39 §5.16 §5.35 — track which window holds the
                // OS keyboard focus for the key-dispatch gate (separate from
                // the FocusManager save/restore below: that owns the focused-
                // widget snapshot, this owns the OS-focus identity keyboard
                // routing keys on).
                self.core.note_os_focus(spec_id, focused);
                // R51.59 §5.39 — Window blur / refocus. ARIA Focus
                // Order asks the framework to reinstate the focused
                // widget when the user returns to the window.
                if focused {
                    self.core.window_focused();
                } else {
                    self.core.window_blurred();
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                // R1073 PR-27.4 §5.39 — extracted to `handle_keyboard_input` to
                // keep this dispatcher under the workspace
                // `clippy::too_many_lines` (100) ceiling (the app.rs split
                // convention). Passes the `Copy` `window_id` (the helper
                // re-resolves `spec_id` itself) so the keyboard / auto-repeat
                // hot path allocates nothing — unlike the rarer mouse / file
                // arms that `to_owned()` the borrowed `spec_id`.
                // R1076 PR-28 §5.39 — `is_synthetic` (winit focus-transition
                // key-state sync, dropped pre-R1076) gates the dock-toggle flap.
                self.handle_keyboard_input(event_loop, window_id, &event, is_synthetic);
            }
            // R56.2.a §5.13 §5.38 — IME composition events from the
            // platform input method (Wayland `text-input-v3`, X11
            // XIM, macOS `NSTextInputContext`, Windows TSF, GTK
            // IBus — winit 0.30 abstracts all four under one `Ime`
            // enum). Map to the pinion-native
            // [`pinion_core::CompositionEvent`] surface via the
            // `was_composing` state machine and dispatch through
            // `ShellCore::apply_composition` (R56.2.a substrate).
            // Multiple `CompositionEvent`s can fan out of a single
            // `Ime` event (e.g. first non-empty `Preedit` produces
            // `Start + Update`); see [`winit_ime_to_composition`]
            // for the mapping table.
            WindowEvent::Ime(ime) => {
                // R670.B §5.16 — per-window IME state machine. The
                // composition session belongs to the focused window;
                // tracking `was_composing` per-window means a
                // multi-window binding's main + inspector can each
                // carry an independent composition session.
                if let Some(slot) = self.windows.get_mut(&window_id) {
                    let (events, next_state) =
                        winit_ime_to_composition(&ime, slot.ime_was_composing);
                    slot.ime_was_composing = next_state;
                    for event in events {
                        self.core.apply_composition(&event);
                    }
                }
            }
            // R670.B / R1023 / R1123 §5.16 — per-window resize. Extracted to
            // `note_window_resized` to keep `window_event` under the 100-line
            // ceiling (the app.rs split convention) and to let the helper own
            // the slot borrow the maximized-cache sync needs.
            // R1147 §5.51 — the shell-private drag-preview window is not in
            // `windows`, so route its resize to its own surface; otherwise the
            // declared-window resize path.
            WindowEvent::Resized(size) if self.is_drag_preview_window(window_id) => {
                self.note_preview_resized(size);
            }
            WindowEvent::Resized(size) => self.note_window_resized(window_id, size),
            // R1088 §5.16 §5.41 §2 #7 PR-31 — OS window move. Feed the new
            // outer position back into `windows_signal` so the DECLARED
            // position converges on the actual one (a user title-bar drag
            // writes the position SSOT; `scene/windows` then reads it).
            // Echo of the shell's own `set_outer_position` is suppressed
            // inside `note_window_moved`. Extracted (like the scale arm) to
            // keep `window_event` under the 100-line ceiling.
            WindowEvent::Moved(position) => {
                self.note_window_moved(window_id, position);
            }
            // R1027 §5.16 — DPI / scale change. Extracted to
            // `note_scale_factor_changed` to keep this dispatcher under the
            // workspace `clippy::too_many_lines` (100) ceiling (the app.rs
            // split convention, as `handle_mouse_button` / `handle_file_dnd`).
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.note_scale_factor_changed(window_id, scale_factor);
            }
            // R57.1 §5.50 — OS `prefers-color-scheme` change. winit
            // fires this on every desktop platform pinion supports
            // (macOS observes `NSApp.effectiveAppearance`, GNOME /
            // KDE observe `gsettings color-scheme`, Windows observes
            // `ImmersiveColorSet`). Forward to the global
            // `pinion_core::SystemColorScheme` signal so every
            // [`ThemeProvider`] in [`ThemeMode::System`] re-resolves
            // its palette in the next frame.
            WindowEvent::ThemeChanged(theme) => {
                pinion_core::set_system_color_scheme(winit_theme_to_pinion_scheme(theme));
            }
            // R1147 §5.51 — the shell-private drag-preview window paints a fixed
            // chip via its own render path (it is not in `windows`, so
            // `render_window` would early-return for it).
            WindowEvent::RedrawRequested if self.is_drag_preview_window(window_id) => {
                self.render_drag_preview();
            }
            WindowEvent::RedrawRequested => self.render_window(window_id),
            _ => {}
        }
        self.drain_redraw_to_winit();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::RpcRequest(json) => self.dispatch_rpc(&json),
            AppEvent::AccessKit(ak) => self.handle_accesskit_event(ak),
            // R51.159 §5.23 — re-feed an Intent produced by a resolved
            // Command future back into the SCXML `send` channel via
            // `ShellCore::dispatch_intent`. The closing step of the
            // §5.23 R27 dispatch loop.
            AppEvent::IntentArrived(intent) => self.core.dispatch_intent(&intent),
            // R683 §5.16 §5.41 — the reconcile Effect (installed in
            // [`Self::resumed`] when `WidgetView::windows_signal()`
            // returned `Some(..)`) fired this user-event on every
            // value-changing emit of the binding's
            // `Signal<Vec<WindowSpec>>`. The handler re-reads the
            // signal snapshot + diffs against the cached last-known
            // spec list + resumes added specs + drops removed specs.
            // Idempotent on identical re-emits (Vec PartialEq
            // short-circuit inside `reconcile_windows`).
            AppEvent::WindowsDirty => self.reconcile_windows(event_loop),
            // R999 §5.23 — an off-thread producer wrote fresh data into a
            // shared handle the binding's `view` reads. Arm a binding-wide
            // redraw; the `drain_redraw_to_winit` tail below collapses it (and
            // any other wakes this iteration) into one `Window::request_redraw`,
            // and the next frame re-runs `view` which re-reads the handle.
            AppEvent::ExternalRepaint => self.core.request_redraw(),
        }
        self.drain_redraw_to_winit();
    }

    /// R681 §2 #4 atomic 2 §5.16 §5.28 — per-window game-loop pacing.
    /// winit calls `about_to_wait` after every batch of pending events
    /// has been drained and immediately before the event loop blocks
    /// for more input. This is the canonical hook to configure the
    /// next [`ControlFlow`]:
    ///
    /// - Any active window slot whose published
    ///   [`ShellCore::immediate_subtree_for_window`] is `true` arms the
    ///   §2 #4 game-loop branch: compute that slot's
    ///   next-paint deadline as
    ///   `slot.last_paint_instant + frame_budget`
    ///   (`frame_budget` = 1/60s; per-window override lands in
    ///   atomic 3 via
    ///   [`pinion_runtime::frame_pacing::frame_budget_for_window`]).
    /// - The earliest deadline across every immediate-mode slot wins
    ///   — winit has a single global control-flow setting per loop
    ///   iteration, so the tightest deadline determines when winit
    ///   wakes up; the per-window paint clock survives because each
    ///   window's [`Self::render_window`] still reads its own
    ///   `last_paint_instants` slot for `dt`, and the
    ///   `redraw_requested_for_window` flag fires only the affected
    ///   window's redraw.
    /// - When no slot has an immediate-mode subtree, fall back to
    ///   [`ControlFlow::Wait`] — the input-driven retained-tree
    ///   semantics every Phase A binding already relies on.
    ///
    /// Each immediate-mode slot also re-arms its per-window redraw
    /// flag here so the next event-loop iteration's
    /// [`Self::drain_redraw_to_winit`] dispatches one
    /// `Window::request_redraw` per slot — that delivers the
    /// `WindowEvent::RedrawRequested` event the slot's
    /// [`Self::render_window`] consumes to drive frame N+1. Without
    /// this re-arm the §2 #4 game-loop would stall after the first
    /// paint (the substrate's compute-paint-scene wire arms the flag
    /// on first paint, but only one drain consumes it; subsequent
    /// frames need the re-arm here).
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let mut earliest_deadline: Option<Instant> = None;
        // R683 §5.16 — `slot.spec_id` is `Cow<'static, str>` which
        // can wrap a runtime-generated id (`Cow::Owned`) for the dock
        // tear-off arc. The per-iteration `slot_ids` buffer carries
        // owned `Cow`s so the substrate calls below survive the
        // `self.windows.values()` borrow release.
        let mut immediate_slot_ids: Vec<Cow<'static, str>> = Vec::new();
        for slot in self.windows.values() {
            if !matches!(slot.render, RenderState::Active { .. }) {
                continue;
            }
            // R681 atomic 3 — derive the per-window frame budget from
            // the substrate signals (both homed on `ShellCore`): the
            // sticky `immediate_subtree` flag `render_window` publishes
            // + the binding's optional `set_target_fps_for_window`
            // override. `None` budget means "this slot does not
            // contribute a deadline" (idle policy, the default for
            // retained-tree windows). The §5.16 jank profiler reads the
            // same two signals through the same helper, so its budget
            // equals this pacing budget for every window.
            let override_fps = self.core.target_fps_for_window(&slot.spec_id);
            let budget = pinion_runtime::frame_pacing::frame_budget_for_window(
                self.core.immediate_subtree_for_window(&slot.spec_id),
                override_fps,
            );
            let Some(budget) = budget else { continue };
            immediate_slot_ids.push(slot.spec_id.clone());
            let deadline = match self.core.last_paint_instant_for_window(&slot.spec_id) {
                Some(prev) => prev + budget,
                // No prior paint — schedule ASAP (now + 0). winit
                // clamps WaitUntil(past_or_present) to "wake
                // immediately" semantics.
                None => now,
            };
            earliest_deadline = Some(match earliest_deadline {
                Some(d) => d.min(deadline),
                None => deadline,
            });
        }
        for spec_id in immediate_slot_ids {
            // Re-arm so the next iteration's
            // `drain_redraw_to_winit` dispatches the per-window
            // `Window::request_redraw`. Idempotent: only one redraw
            // per drain regardless of how many times we set the flag.
            self.core.request_redraw_for_window(&spec_id);
        }
        // R924 §5.22 — keep the event loop awake while the owner-scoped
        // `LocalTaskPump` has async work in flight (a `Resource` fetch
        // spawned by a reactive `Effect` — e.g. lazy page loading driven by
        // a scroll). `compute_paint_scene_internal` polls the pump and
        // re-arms `redraw_requested` while pending, so the loop self-sustains
        // *once a frame renders*; this requests the frame that bootstraps it.
        // It is needed because some RPCs mutate reactive state — and so spawn
        // pump work — without arming a redraw (e.g. `scene/scroll`, whose
        // offset write fires the prefetch `Effect`): absent this, the loop
        // would sleep at `ControlFlow::Wait` (the deadline above is computed
        // only from animations + immediate-mode subtrees, not the pump) and
        // the fetch would stall. This is the "stay awake while active"
        // contract the `LocalTaskPump` doc names. NOTE the cost: while a task
        // is pending the scene re-renders every frame (the v1 `Waker::noop`
        // poll model — see `LocalTaskPump` docs); a wake-channel waker that
        // re-renders only on task progress is the documented forward
        // refinement (R761.1 carry) for genuinely long-running fetches.
        if self.core.root_owner().local_task_pump().has_pending() {
            self.core.request_redraw();
            earliest_deadline = Some(earliest_deadline.map_or(now, |d| d.min(now)));
        }
        match earliest_deadline {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
        // Drain the per-window flags we just armed so the
        // immediate-mode windows receive their next redraw without
        // waiting on another event-loop cycle.
        self.drain_redraw_to_winit();
    }
}

/// R907 §5.16 §5.7 — microseconds elapsed between two `Instant`s for
/// the frame-timing profiler, saturating to `u64::MAX`.
/// `saturating_duration_since` guards the (monotonic-clock-impossible)
/// `end < start` case; the `u128 → u64` cast saturates a frame longer
/// than ~584,000 years, which keeps clippy + the type honest without a
/// real overflow path.
fn instant_delta_us(start: Instant, end: Instant) -> u64 {
    u64::try_from(end.saturating_duration_since(start).as_micros()).unwrap_or(u64::MAX)
}

/// R51.37 §5.35 R1009 §5.13 — bridge from winit's [`NamedKey`] enum to the
/// W3C-aligned `KeyboardEvent.key` strings the
/// [`WidgetView::apply_key`](crate::WidgetView) contract speaks.
///
/// Surfaces the keys with an established cross-platform **widget** meaning:
/// navigation (arrows / Home / End / Page), activation (Enter / Space), the
/// **editing** keys (Backspace / Delete / Insert) and the **function** row
/// (F1–F12). R1009 added the editing + function keys for the content-surface
/// consumer — a terminal / code-editor / canvas forwards every one of them to
/// its child (sprag's PTY pane is the forcing case); the earlier curation
/// dropped them as "no widget cares", which is true only of the device-control
/// keys (`Browser*` / `Media*` / `Launch*` / `Audio*`). Those stay `None`: no
/// widget interprets them, and one that did would key off `apply_key` returning
/// `false` either way, so surfacing them buys nothing.
///
/// `NamedKey::Escape` / `NamedKey::Tab` are filtered **upstream** of this
/// bridge (the shell-reserved quit / `FocusManager` traverse arms in
/// [`AppShell::handle_key_press`] offer them to the focused widget first), so
/// they intentionally return `None` here.
///
/// The TUI backend's peer bridge `pinion_tui::input::key_str_from_event`
/// (crossterm `KeyCode` → the same W3C strings) must surface the same
/// content-surface vocabulary. The two map different platform enums, so the
/// tables are **not** folded (the R773 pin-don't-fold rule); their shared canon
/// is the W3C `KeyboardEvent.key` spec — keep both pinned to it. R1009 widened
/// this winit table to the editing + function coverage the TUI peer already had
/// (the drift that absence proved).
///
/// R51.92.1 §5.40 — module-local helper (sole caller is
/// [`AppShell::handle_key_press`] above).
fn named_key_str(named: NamedKey) -> Option<&'static str> {
    match named {
        NamedKey::ArrowLeft => Some("ArrowLeft"),
        NamedKey::ArrowRight => Some("ArrowRight"),
        NamedKey::ArrowUp => Some("ArrowUp"),
        NamedKey::ArrowDown => Some("ArrowDown"),
        NamedKey::Home => Some("Home"),
        NamedKey::End => Some("End"),
        NamedKey::PageUp => Some("PageUp"),
        NamedKey::PageDown => Some("PageDown"),
        NamedKey::Enter => Some("Enter"),
        NamedKey::Space => Some("Space"),
        // R1009 §5.13 — editing keys: a content-surface widget forwards these to
        // its child (Backspace = 0x7f, Delete = ESC[3~ in a PTY).
        NamedKey::Backspace => Some("Backspace"),
        NamedKey::Delete => Some("Delete"),
        NamedKey::Insert => Some("Insert"),
        // R1009 §5.13 — the function row F1–F12 (each an xterm escape). winit
        // also exposes F13–F35, left out until a consumer needs them.
        NamedKey::F1 => Some("F1"),
        NamedKey::F2 => Some("F2"),
        NamedKey::F3 => Some("F3"),
        NamedKey::F4 => Some("F4"),
        NamedKey::F5 => Some("F5"),
        NamedKey::F6 => Some("F6"),
        NamedKey::F7 => Some("F7"),
        NamedKey::F8 => Some("F8"),
        NamedKey::F9 => Some("F9"),
        NamedKey::F10 => Some("F10"),
        NamedKey::F11 => Some("F11"),
        NamedKey::F12 => Some("F12"),
        _ => None,
    }
}

/// R1073.1 PR-27.4 §5.39 — the DISPATCH key vocabulary for the press-owner gate
/// ([`crate::ShellCore::admit_key_press`]): a superset of [`named_key_str`] that
/// adds the two shell-reserved keys [`named_key_str`] deliberately excludes from
/// its R1009 content-surface / chord vocabulary — `Escape` and `Tab`. Those are
/// not forwarded to a content surface and are not chord keys, but
/// [`AppShell::handle_key_press`] DOES dispatch them (Escape → modal-cancel /
/// `event_loop.exit`, Tab → focus traverse), so the close-during-dispatch gate
/// must see them too: a future `Escape`-closes-a-window binding gets the same
/// one-press-one-action protection as `Enter`. Used ONLY for the owner gate +
/// its release ([`crate::ShellCore::note_key_release`]); the chord cache keeps
/// the narrower [`named_key_str`] vocabulary. `None` only for keys the shell
/// does not dispatch at all (media / browser keys, dead keys).
fn dispatch_named_key_str(named: NamedKey) -> Option<&'static str> {
    match named {
        NamedKey::Escape => Some("Escape"),
        NamedKey::Tab => Some("Tab"),
        other => named_key_str(other),
    }
}

/// Background thread: read JSON-RPC 2.0 lines from stdin and forward
/// each as an `AppEvent::RpcRequest` user event. Blank lines are
/// skipped; EOF or any read error terminates the thread quietly (the
/// GUI loop keeps running). The proxy `send_event` fails only after
/// the event loop has shut down, in which case we also exit the
/// thread.
///
/// R51.92.1 §5.40 — module-local helper (sole caller is [`run`]
/// below).
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

/// R51.108 §5.41 — convert a `winit::event::Touch` to the
/// substrate-local [`pinion_runtime::Touch`] at the winit boundary so
/// `ShellCore` stays winit-free for the §2 #6 GUI/TUI dual invariant.
/// `winit::event::Touch::id` already matches the abstract `id: u64`;
/// `location: PhysicalPosition<f64>` decomposes to `(x, y)`; the
/// four-variant `TouchPhase` enum maps 1:1.
/// R1027 §5.16 — whether a window `scale_factor` is non-identity.
///
/// Gates the scaled vello append (`render_window`) + the AccessKit root
/// transform so a `1.0` (non-`HiDPI`) window stays on the byte-identical
/// pre-R1027 render path. winit reports exact factors (`1.0` / `1.5` /
/// `2.0` …), but a bare `!= 1.0` would trip `clippy::float_cmp`; the
/// `f64::EPSILON` margin excludes only a literal `1.0`.
fn scale_is_non_identity(scale: f64) -> bool {
    (scale - 1.0).abs() > f64::EPSILON
}

/// R1060 §5.16 §5.12 — submit one encoded frame to a window's renderer.
///
/// Normally this is the `render` present; when `capture` is set (an RPC
/// `scene/screenshot` flagged the slot via
/// [`AppShell::capture_window_screenshot`]) it is `capture_rgba8`, which
/// renders AND reads back the presented swapchain texture. Returns the
/// present outcome (fed to the per-window render-fidelity record) plus
/// the captured frame when one was requested. Extracted from
/// [`AppShell::render_window`] so that paint method stays under the
/// workspace `clippy::too_many_lines` ceiling; the normal (`capture ==
/// false`) path is byte-identical to the pre-R1060 inline present.
fn submit_frame<R: VelloRenderer>(
    renderer: &mut R,
    target: &VelloScene,
    base: vello::peniko::Color,
    capture: bool,
) -> (bool, Option<crate::vello_capture::CapturedFrame>) {
    if capture {
        match renderer.capture_rgba8(target, base) {
            Ok(frame) => (true, Some(frame)),
            Err(e) => {
                eprintln!("shell: vello capture: {e}");
                (false, None)
            }
        }
    } else {
        let ok = match renderer.render(target, VelloContext { base_color: base }) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("shell: vello render: {e}");
                false
            }
        };
        (ok, None)
    }
}

/// R1027 §5.16 — convert a winit **physical** `inner_size` to the
/// **logical** layout size the paint scene is built in.
///
/// Since R1027 the whole pinion scene / layout / introspection world is
/// logical; physical pixels appear only at the GPU surface size, the
/// vello raster scale, and pointer input. Delegates to winit's own DPI
/// math ([`PhysicalSize::to_logical`]) rather than hand-rolling the
/// division + rounding.
///
/// May return `0` for a degenerate (minimized / sub-logical-pixel)
/// dimension — e.g. a 1-physical-px width at scale 4 rounds to `0`
/// logical. The caller's `NonZeroU32` guard in `render_window`
/// early-returns on a `0` dimension (no paint), exactly as pre-R1027
/// did when it fed the raw physical `inner_size` to `NonZeroU32::new`.
/// This is deliberately NOT clamped to `>= 1` here: clamping would make
/// that guard unreachable and paint a wasted 1px frame for a 0-size
/// window.
///
/// `scale` must be a valid winit factor (positive + normal); winit's
/// `to_logical` asserts this. Always satisfied here — the only callers
/// pass `WindowSlot::scale_factor`, sourced from `Window::scale_factor()`
/// / `ScaleFactorChanged`.
fn logical_layout_size(physical: PhysicalSize<u32>, scale: f64) -> (u32, u32) {
    let logical: LogicalSize<u32> = physical.to_logical(scale);
    (logical.width, logical.height)
}

/// R1027 §5.16 §5.35 — convert a winit **physical** pointer position to
/// the **logical** coordinate space the [`pinion_runtime::InputRouter`]
/// hit-tests in (the same space the logical paint scene lays out in).
///
/// Without this, a 4-logical-px hit target (e.g. a splitter handle) on a
/// 2x display would be a 4-device-px target the router never resolves at
/// the cursor's true logical position. Delegates to winit's
/// [`PhysicalPosition::to_logical`] (the canonical platform DPI math).
/// `scale` must be a valid winit factor (positive + normal), which the
/// per-slot `scale_factor` always is.
fn winit_pointer_to_logical(pos: PhysicalPosition<f64>, scale: f64) -> (f64, f64) {
    let logical: LogicalPosition<f64> = pos.to_logical(scale);
    (logical.x, logical.y)
}

fn winit_touch_to_pinion(touch: winit::event::Touch, scale: f64) -> pinion_runtime::Touch {
    // R1027 §5.16 §5.35 — touch location is physical; map to logical so
    // it shares the router's coordinate space (mirrors `CursorMoved`).
    let (x, y) = winit_pointer_to_logical(touch.location, scale);
    pinion_runtime::Touch {
        id: touch.id,
        x,
        y,
        phase: match touch.phase {
            winit::event::TouchPhase::Started => pinion_runtime::TouchPhase::Started,
            winit::event::TouchPhase::Moved => pinion_runtime::TouchPhase::Moved,
            winit::event::TouchPhase::Ended => pinion_runtime::TouchPhase::Ended,
            winit::event::TouchPhase::Cancelled => pinion_runtime::TouchPhase::Cancelled,
        },
    }
}

/// (R51.186 §5.45 R55.C.2) Convert a winit `MouseScrollDelta` to
/// the substrate-local [`WheelDelta`] at the winit boundary.
///
/// winit reports wheel input in one of two unit modes —
/// `LineDelta(f32, f32)` for legacy notched mouse wheels and
/// `PixelDelta(PhysicalPosition<f64>)` for trackpad inertia /
/// high-resolution scroll. Both narrow into pinion's unit-tagged
/// variants.
///
/// R1027 §5.16 §5.45 — `PixelDelta` is in **physical** device pixels
/// (winit `MouseScrollDelta` doc), so it is divided by `scale` to the
/// logical coordinate space the scene + `ScrollState` live in — the
/// pointer-input boundary (#3) of the R1027 logical-coordinate policy.
/// Without this a 2x display would over-scroll the logical content 2x.
/// `LineDelta` is unitless notch counts (not pixels), so it is
/// scale-independent and left unscaled. The `f64` delta narrows to
/// `f32` after the divide (the substrate consumes the delta at `f32` —
/// `External::wheel` directly, the `ScrollState` fallback after an
/// `i32` round — so the wider `f64` carries no information past here).
///
/// (R51.192 §5.45 R55.C.2) Both axes flip sign at this boundary.
/// winit's [`MouseScrollDelta`] convention is "positive = content
/// being scrolled should move right and down (revealing more
/// content left and up)" — i.e. winit `(dx, dy) > 0` means the
/// scroll is toward the content origin, the user wants to *see*
/// what is above/left of the current viewport. pinion's substrate
/// (and the [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by)
/// it forwards into) follows the W3C `WheelEvent.deltaY` /
/// `deltaX` convention — positive scrolls *away* from the
/// content origin, exposing what is below/right. The two
/// conventions are opposite-signed, so the boundary flip lands
/// here exactly once. The TUI sibling
/// [`pinion_tui::shell::dispatch_mouse`] already emits W3C-signed
/// `WheelDelta` values (`ScrollUp` → `dy = -1.0`,
/// `ScrollDown` → `dy = +1.0`), so the substrate stays
/// crossterm + W3C agreed and only winit needs the flip.
#[allow(clippy::cast_possible_truncation)]
fn winit_wheel_to_pinion(delta: MouseScrollDelta, scale: f64) -> WheelDelta {
    match delta {
        MouseScrollDelta::LineDelta(dx, dy) => WheelDelta::Lines { dx: -dx, dy: -dy },
        MouseScrollDelta::PixelDelta(pos) => {
            // R1027 §5.16 §5.45 — physical device pixels -> logical, same
            // conversion as `CursorMoved` / `Touch` (reuses the shared
            // `winit_pointer_to_logical` helper), so high-res / trackpad
            // scrolling moves the logical content the intended distance.
            let (lx, ly) = winit_pointer_to_logical(pos, scale);
            WheelDelta::Pixels {
                dx: -(lx as f32),
                dy: -(ly as f32),
            }
        }
    }
}

/// R51.108 §5.41 — convert a `winit::keyboard::ModifiersState` to the
/// substrate-local [`pinion_runtime::Modifiers`] at the winit boundary.
/// The four W3C DOM Level 3 modifier bits map 1:1 (winit's `super_key`
/// is the Meta / Cmd / Win key in the abstract vocabulary).
fn winit_modifiers_to_pinion(
    modifiers: winit::keyboard::ModifiersState,
) -> pinion_runtime::Modifiers {
    pinion_runtime::Modifiers {
        shift: modifiers.shift_key(),
        ctrl: modifiers.control_key(),
        alt: modifiers.alt_key(),
        meta: modifiers.super_key(),
    }
}

/// R57.1 §5.50 — translate winit's two-state
/// [`winit::window::Theme`](winit::window::Theme) (`Light` / `Dark`)
/// into the W3C-aligned three-state
/// [`pinion_core::SystemColorScheme`]. winit itself never surfaces a
/// "no preference" reading on `Window::theme()` or
/// `WindowEvent::ThemeChanged` (every desktop backend — macOS
/// `NSApp.effectiveAppearance`, GNOME / KDE `gsettings color-scheme`,
/// Windows `ImmersiveColorSet` — resolves the OS signal to one of
/// the two), so this helper maps 1:1 onto `Light` / `Dark`. The
/// `NoPreference` variant remains the fresh-thread default; the
/// platform side never *clears* the signal back to it.
fn winit_theme_to_pinion_scheme(theme: winit::window::Theme) -> pinion_core::SystemColorScheme {
    match theme {
        winit::window::Theme::Light => pinion_core::SystemColorScheme::Light,
        winit::window::Theme::Dark => pinion_core::SystemColorScheme::Dark,
    }
}

/// R56.2.a §5.13 §5.38 — map a `winit::event::Ime` (cross-platform IME
/// abstraction covering Wayland `text-input-v3` + X11 XIM + macOS
/// `NSTextInputContext` + Windows TSF + GTK `IBus`) onto the pinion-
/// native [`pinion_core::CompositionEvent`] sequence the substrate
/// consumes (`Start` / `Update` / `Commit` / `Cancel`).
///
/// The mapping is deterministic given the current `was_composing`
/// boolean (the shell-side state machine):
///
/// | winit event                        | `was_composing` in  | Dispatched `CompositionEvent`s          | `was_composing` out |
/// |------------------------------------|---------------------|-----------------------------------------|---------------------|
/// | `Ime::Enabled`                     | any                 | (none — just a notification)            | unchanged           |
/// | `Ime::Preedit(text, _)` non-empty  | `false`             | `Start`, `Update(text)`                 | `true`              |
/// | `Ime::Preedit(text, _)` non-empty  | `true`              | `Update(text)`                          | `true`              |
/// | `Ime::Preedit("", _)` empty        | `false`             | (none — idempotent)                     | `false`             |
/// | `Ime::Preedit("", _)` empty        | `true`              | `Update("")` (visual clear, stay open)  | `true`              |
/// | `Ime::Commit(text)`                | `false`             | `Start`, `Commit(text)`                 | `false`             |
/// | `Ime::Commit(text)`                | `true`              | `Commit(text)`                          | `false`             |
/// | `Ime::Disabled`                    | `false`             | (none — idempotent)                     | `false`             |
/// | `Ime::Disabled`                    | `true`              | `Cancel`                                | `false`             |
///
/// Key invariants behind the table:
///
/// 1. **Empty `Preedit` is `Update("")`, not `Cancel`** — winit
///    documents "Right before [`Commit`] event winit will send empty
///    `Self::Preedit` event" as a synthetic clear. Treating empty
///    preedit as cancel would fire a spurious `Cancel + Commit` pair
///    on every pinyin / Hangul commit. Instead the visual clears and
///    the substrate stays composing; on the immediately-following
///    `Commit` the substrate (still in `Editing`) commits at the
///    caret with the `was_composing && !text.is_empty()` gate
///    intact. (R56.1.g.1 substrate.)
///
/// 2. **`Commit` from `was_composing=false` injects a synthetic
///    `Start`** — macOS dead-key sequences and trivial-IME paths
///    can emit `Commit` without a prior `Preedit`. Seeding `Start`
///    drives the SCXML through `Focused → Editing` so the substrate
///    is consistent (`preedit_buffer` switches `None → Some("")`)
///    before the immediate `Commit` lands the text. Without the
///    synthetic `Start` the text would still insert at caret (the
///    `preedit_commit` path runs unconditionally) but the SCXML
///    would never reach `Editing` and the `text_committed` intent
///    would not fire (`was_composing` gate evaluates to `false`).
///
/// 3. **`Disabled` mid-session dispatches `Cancel`, not just
///    state reset** — the substrate observes the cancel through
///    SCXML `CancelEdit` drive + `preedit_cancel`; without the
///    explicit `Cancel` the caret blink would stay paused (R56.1.j)
///    and the `Editing` SCXML state would linger.
///
/// 4. **`Enabled` is informational** — winit emits `Enabled` after
///    `set_ime_allowed(true)` succeeds, but the substrate's IME
///    state is driven entirely by the `Preedit` / `Commit` /
///    `Disabled` triplet. `Enabled` is a hook a future sub-round
///    could use for `set_ime_cursor_area` rebroadcast on session
///    start.
///
/// Returns the dispatched events as a `Vec` (caller iterates in
/// order) plus the next `was_composing` state for the caller to
/// store on the shell. Free function (not a method) so unit tests
/// can drive the mapping table without a winit `EventLoop`.
fn winit_ime_to_composition(
    ime: &Ime,
    was_composing: bool,
) -> (Vec<pinion_core::CompositionEvent>, bool) {
    use pinion_core::CompositionEvent;
    match ime {
        Ime::Enabled => (Vec::new(), was_composing),
        Ime::Preedit(text, _cursor) => {
            if text.is_empty() {
                if was_composing {
                    (vec![CompositionEvent::Update(String::new())], true)
                } else {
                    (Vec::new(), false)
                }
            } else if was_composing {
                (vec![CompositionEvent::Update(text.clone())], true)
            } else {
                (
                    vec![
                        CompositionEvent::Start,
                        CompositionEvent::Update(text.clone()),
                    ],
                    true,
                )
            }
        }
        Ime::Commit(text) => {
            if was_composing {
                (vec![CompositionEvent::Commit(text.clone())], false)
            } else {
                (
                    vec![
                        CompositionEvent::Start,
                        CompositionEvent::Commit(text.clone()),
                    ],
                    false,
                )
            }
        }
        Ime::Disabled => {
            if was_composing {
                (vec![CompositionEvent::Cancel], false)
            } else {
                (Vec::new(), false)
            }
        }
    }
}

/// R1087 §5.16 §5.41 PR-31 — the pure **move-pass** diff for
/// [`AppShell::reconcile_windows`]: which already-open windows must have
/// their OS position reconciled because the binding re-declared a
/// different [`WindowSpec::position`].
///
/// A window appears here iff its `id` is present in **both** `old` and
/// `new` (so it is neither an add nor a drop — those passes key on id
/// alone) AND `new` declares a position (`Some`) that differs from what
/// `old` declared (`old.position != Some(new_pos)`, which also fires when
/// `old` left placement to the window manager — `None` → first declared
/// position). A `new` spec that drops back to `None` is **not** a move:
/// `set_outer_position` cannot hand a window back to WM auto-placement, so
/// the declared `None` simply leaves the window where it is.
///
/// Splitting this out of `reconcile_windows` keeps the genuinely-new logic
/// pure and unit-testable with no winit event loop (the apply —
/// `Window::set_outer_position` — stays in the imperative reconcile, its
/// live effect HW-gated like every other real-window behaviour).
///
/// **This DIFF is total** over the `position` field: every same-id position
/// change appears in the returned `Vec` (pre-R1087 the add/drop passes would
/// silently swallow it — the top-level `new_specs == old_specs` guard sees
/// the diff, then neither id-keyed pass acts on it). The *apply* the caller
/// then performs is best-effort per window: a window with a live arc moves
/// immediately; a window with no live arc — a `Suspended(Some)` mobile state
/// ([`crate::AppShell::slot_window`] returns `None`) — reconciles on its next
/// create instead, not here (a mobile-lifecycle gap deferred with mobile,
/// not a desktop one). So "total" describes the diff, not the OS effect.
///
/// `old.position != Some(new_pos)` also fires when `old` left placement to the
/// window manager (`None` → first declared position). A `new` spec that drops
/// back to `None` is **not** a move: `set_outer_position` cannot hand a window
/// back to WM auto-placement, so the declared `None` leaves the window where
/// it is.
///
/// O(N²) (`find` inside the `new` loop) unlike the `HashSet` add/drop passes:
/// deliberately accepted — N is the window count (a handful), so a map would
/// be premature; the add/drop passes go `HashSet` only because their set
/// difference can span many ids.
fn window_position_moves(old: &[WindowSpec], new: &[WindowSpec]) -> Vec<(String, (i32, i32))> {
    let mut moves = Vec::new();
    for spec in new {
        let Some(new_pos) = spec.position else {
            continue;
        };
        // Only an id present in `old` is a move — an id only in `new` is
        // an ADD (resume_spec applies its initial position via
        // `with_position`, not this pass).
        if let Some(old_spec) = old.iter().find(|o| o.id == spec.id) {
            if old_spec.position != Some(new_pos) {
                moves.push((spec.id.as_ref().to_owned(), new_pos));
            }
        }
    }
    moves
}

/// R1088 §5.16 §5.41 §2 #7 PR-31 — is this `WindowEvent::Moved` the echo of
/// a position the shell just commanded via `set_outer_position`? `true`
/// when `commanded` is set and equals the incoming `logical` position
/// within 1px.
///
/// R1091: the 1px tolerance is NOT for logical<->physical rounding — that
/// round-trip is EXACT for an integer-logical command (`round(round(L*s)/s)
/// == L` for every integer `L` across DPI scales). It absorbs **window-
/// manager placement imprecision**: `set_outer_position` is a REQUEST a WM
/// may honour off-by-a-pixel (edge-snapping, decoration insets, tiling
/// quantization), so the OS `Moved` echo can land 1px off our command
/// without being a user drag — and treating that as a user move would
/// re-write the signal and risk a command/echo nudge loop. Cost: a
/// deliberate 1px user nudge of a just-commanded window is misclassified as
/// an echo and swallowed, which is below intent resolution and acceptable.
///
/// The pure core of [`AppShell::note_window_moved`]'s echo suppression,
/// split out so the own-command-vs-user-drag classification is unit-testable
/// without a live winit event loop (the `Moved` delivery + `set_outer_position`
/// effect stay HW-gated, like every real-window behaviour).
fn moved_is_command_echo(commanded: Option<(i32, i32)>, logical: (i32, i32)) -> bool {
    matches!(
        commanded,
        Some((cx, cy)) if (logical.0 - cx).abs() <= 1 && (logical.1 - cy).abs() <= 1
    )
}

/// R1088/R1091 §5.16 §5.41 §2 #7 PR-31 — the pure write-back core of
/// [`AppShell::note_window_moved`]: given the current declared `specs`, the
/// moved window's `spec_id`, and its new `logical` position, return the
/// updated `specs` to emit, or `None` to skip.
///
/// Conservative scope: only a window that ALREADY declares a position is
/// written (a `None` WM-placed window — the typical `"main"` — is left
/// WM-managed; one user drag must not silently pin it). A missing id, an
/// id whose spec has no declared position, or an unchanged position all
/// return `None` (no emit). Split out (R1091, mirroring `moved_is_command_echo`
/// and `window_position_moves`) so the conservative-scope filter + the
/// idempotency skip are unit-tested without a live `Moved` event — the OS
/// delivery and `signal.set` effect are the only HW-gated parts.
fn user_move_writeback(
    mut specs: Vec<WindowSpec>,
    spec_id: &str,
    logical: (i32, i32),
) -> Option<Vec<WindowSpec>> {
    let spec = specs
        .iter_mut()
        .find(|s| s.id.as_ref() == spec_id && s.position.is_some())?;
    if spec.position == Some(logical) {
        return None;
    }
    spec.position = Some(logical);
    Some(specs)
}

#[cfg(test)]
mod r1087_window_position_move_diff_tests {
    //! R1087 §5.16 §5.41 PR-31 — the pure `window_position_moves` diff
    //! that drives `reconcile_windows`'s move pass. Forcing consumer for
    //! the move logic (the live drag-follow runtime consumer is the next
    //! PR-31 slice) per the test-as-forcing-consumer discipline.
    use super::{WindowSpec, window_position_moves};
    use crate::SizeStrategy;

    fn fixed(id: &'static str) -> WindowSpec {
        WindowSpec::new(
            id,
            id,
            SizeStrategy::Fixed {
                width: 100,
                height: 100,
            },
        )
    }

    #[test]
    fn unchanged_specs_yield_no_moves() {
        let a = vec![fixed("main"), fixed("torn-x").with_position(40, 50)];
        // Same lists → the idempotent fast-path never reaches the move
        // pass, but the diff itself must also report nothing.
        assert_eq!(window_position_moves(&a, &a), Vec::new());
    }

    #[test]
    fn same_id_position_change_is_a_move() {
        let old = vec![fixed("main"), fixed("torn-x").with_position(40, 50)];
        let new = vec![fixed("main"), fixed("torn-x").with_position(120, 80)];
        assert_eq!(
            window_position_moves(&old, &new),
            vec![("torn-x".to_owned(), (120, 80))]
        );
    }

    #[test]
    fn first_declared_position_on_existing_window_is_a_move() {
        // Window existed WM-placed (None); binding now pins a position.
        let old = vec![fixed("main"), fixed("torn-x")];
        let new = vec![fixed("main"), fixed("torn-x").with_position(10, 10)];
        assert_eq!(
            window_position_moves(&old, &new),
            vec![("torn-x".to_owned(), (10, 10))]
        );
    }

    #[test]
    fn add_with_position_is_not_a_move() {
        // `torn-x` is only in `new` → an ADD (resume_spec places it), not
        // a move. The move pass must leave it to the add pass.
        let old = vec![fixed("main")];
        let new = vec![fixed("main"), fixed("torn-x").with_position(10, 10)];
        assert_eq!(window_position_moves(&old, &new), Vec::new());
    }

    #[test]
    fn dropping_back_to_none_is_not_a_move() {
        // A re-declared `None` cannot un-position a live window
        // (set_outer_position has no "hand back to WM" form), so it is
        // deliberately not reported as a move.
        let old = vec![fixed("torn-x").with_position(40, 50)];
        let new = vec![fixed("torn-x")];
        assert_eq!(window_position_moves(&old, &new), Vec::new());
    }

    #[test]
    fn multiple_simultaneous_moves_all_reported_in_new_order() {
        let old = vec![
            fixed("a").with_position(0, 0),
            fixed("b").with_position(0, 0),
            fixed("c").with_position(5, 5),
        ];
        let new = vec![
            fixed("a").with_position(1, 1),
            fixed("b").with_position(2, 2),
            fixed("c").with_position(5, 5), // unchanged
        ];
        assert_eq!(
            window_position_moves(&old, &new),
            vec![("a".to_owned(), (1, 1)), ("b".to_owned(), (2, 2))]
        );
    }
}

#[cfg(test)]
mod r1088_moved_echo_tests {
    //! R1088 §5.16 §5.41 §2 #7 PR-31 — the pure echo-vs-user-move
    //! classification (`moved_is_command_echo`) + the write-back core
    //! (`user_move_writeback`, R1091) that together gate
    //! `note_window_moved`. The live `Moved` delivery is HW-gated; these are
    //! the testable decision cores.
    use super::{WindowSpec, moved_is_command_echo, user_move_writeback};
    use crate::SizeStrategy;

    fn spec(id: &'static str, pos: Option<(i32, i32)>) -> WindowSpec {
        let s = WindowSpec::new(
            id,
            id,
            SizeStrategy::Fixed {
                width: 100,
                height: 100,
            },
        );
        match pos {
            Some((x, y)) => s.with_position(x, y),
            None => s,
        }
    }

    #[test]
    fn no_command_is_a_user_move() {
        // Latch empty (a WM-placed window the shell never commanded) →
        // never an echo; every Moved is a user/WM move.
        assert!(!moved_is_command_echo(None, (100, 80)));
    }

    #[test]
    fn exact_match_is_an_echo() {
        assert!(moved_is_command_echo(Some((120, 90)), (120, 90)));
    }

    #[test]
    fn one_pixel_off_is_still_an_echo() {
        // R1091: a WM may honour set_outer_position 1px off (snap / inset);
        // that is still our own command echoing, not a user drag.
        assert!(moved_is_command_echo(Some((120, 90)), (121, 89)));
        assert!(moved_is_command_echo(Some((120, 90)), (119, 91)));
    }

    #[test]
    fn two_pixels_off_is_a_user_move() {
        // Beyond the WM-imprecision tolerance → a genuine user/WM drag.
        assert!(!moved_is_command_echo(Some((120, 90)), (122, 90)));
        assert!(!moved_is_command_echo(Some((120, 90)), (120, 88)));
    }

    #[test]
    fn divergent_position_is_a_user_move() {
        assert!(!moved_is_command_echo(Some((120, 90)), (400, 300)));
    }

    // ── user_move_writeback (R1091 S2) — the conservative-scope + ──────
    //    idempotency write-back core, previously untested.

    #[test]
    fn writeback_writes_an_already_positioned_window() {
        let specs = vec![spec("main", None), spec("torn-x", Some((40, 50)))];
        let out =
            user_move_writeback(specs, "torn-x", (200, 130)).expect("a positioned window writes");
        let torn = out.iter().find(|s| s.id.as_ref() == "torn-x").unwrap();
        assert_eq!(torn.position, Some((200, 130)));
        // The other window is untouched.
        assert_eq!(
            out.iter()
                .find(|s| s.id.as_ref() == "main")
                .unwrap()
                .position,
            None
        );
    }

    #[test]
    fn writeback_skips_a_wm_placed_none_window() {
        // Conservative scope: a None (WM-placed) window must NOT be pinned by
        // one user drag → None (no emit).
        let specs = vec![spec("main", None)];
        assert!(user_move_writeback(specs, "main", (300, 220)).is_none());
    }

    #[test]
    fn writeback_skips_unchanged_position() {
        // Idempotent: already at the moved position → None (no churn).
        let specs = vec![spec("torn-x", Some((40, 50)))];
        assert!(user_move_writeback(specs, "torn-x", (40, 50)).is_none());
    }

    #[test]
    fn writeback_skips_missing_id() {
        let specs = vec![spec("torn-x", Some((40, 50)))];
        assert!(user_move_writeback(specs, "ghost", (10, 10)).is_none());
    }
}

#[cfg(test)]
mod r56_2_a_winit_ime_mapping_tests {
    //! R56.2.a §5.13 §5.38 — `winit_ime_to_composition` mapping
    //! regression. Drives the function table (winit `Ime` × shell
    //! `was_composing` → dispatched `CompositionEvent` sequence +
    //! next state) so the bridge between winit's four-variant IME
    //! abstraction and the W3C-aligned pinion-native enum is pinned
    //! end-to-end without needing a real winit `EventLoop`.

    use super::winit_ime_to_composition;
    use pinion_core::CompositionEvent;
    use winit::event::Ime;

    #[test]
    fn r56_2_a_enabled_is_no_op() {
        let (events, next) = winit_ime_to_composition(&Ime::Enabled, false);
        assert!(events.is_empty(), "Enabled is informational, no dispatch");
        assert!(!next, "Enabled leaves was_composing unchanged");

        let (events, next) = winit_ime_to_composition(&Ime::Enabled, true);
        assert!(events.is_empty());
        assert!(next, "Enabled mid-session does not reset was_composing");
    }

    #[test]
    fn r56_2_a_first_nonempty_preedit_dispatches_start_then_update() {
        let (events, next) =
            winit_ime_to_composition(&Ime::Preedit("ha".to_owned(), Some((2, 2))), false);
        assert_eq!(
            events,
            vec![
                CompositionEvent::Start,
                CompositionEvent::Update("ha".to_owned())
            ],
        );
        assert!(next, "first non-empty preedit opens a composition session");
    }

    #[test]
    fn r56_2_a_subsequent_nonempty_preedit_dispatches_update_only() {
        let (events, next) =
            winit_ime_to_composition(&Ime::Preedit("han".to_owned(), Some((3, 3))), true);
        assert_eq!(events, vec![CompositionEvent::Update("han".to_owned())]);
        assert!(next, "subsequent preedit keeps the session open");
    }

    #[test]
    fn r56_2_a_empty_preedit_while_composing_is_update_empty_not_cancel() {
        // The synthetic-clear winit injects right before every
        // Commit. Treating it as Cancel would fire a spurious
        // Cancel+Commit pair on every pinyin / Hangul commit; the
        // substrate stays composing via `Update("")` so the
        // immediately-following Commit lands at the caret with
        // `was_composing` still true.
        let (events, next) = winit_ime_to_composition(&Ime::Preedit(String::new(), None), true);
        assert_eq!(events, vec![CompositionEvent::Update(String::new())]);
        assert!(next, "empty preedit during session keeps was_composing");
    }

    #[test]
    fn r56_2_a_empty_preedit_while_idle_is_idempotent_no_op() {
        let (events, next) = winit_ime_to_composition(&Ime::Preedit(String::new(), None), false);
        assert!(events.is_empty());
        assert!(!next);
    }

    #[test]
    fn r56_2_a_commit_during_session_dispatches_commit_and_closes_session() {
        // Pinyin / Hangul canonical sequence: …Preedit("han") →
        // Preedit("", None) [synthetic clear, dispatched as
        // Update("")] → Commit("\u{D55C}") lands here with
        // was_composing=true.
        let (events, next) = winit_ime_to_composition(&Ime::Commit("\u{D55C}".to_owned()), true);
        assert_eq!(
            events,
            vec![CompositionEvent::Commit("\u{D55C}".to_owned())]
        );
        assert!(!next, "Commit closes the session");
    }

    #[test]
    fn r56_2_a_commit_without_session_injects_synthetic_start() {
        // macOS dead-key sequences emit Commit without a prior
        // Preedit. Inject a synthetic Start so the substrate drives
        // through Focused → Editing and the `was_composing` gate
        // inside `apply_composition_commit` fires the
        // `text_committed` intent.
        let (events, next) = winit_ime_to_composition(&Ime::Commit("e\u{301}".to_owned()), false);
        assert_eq!(
            events,
            vec![
                CompositionEvent::Start,
                CompositionEvent::Commit("e\u{301}".to_owned()),
            ],
        );
        assert!(!next, "Commit always closes the session");
    }

    #[test]
    fn r56_2_a_disabled_mid_session_dispatches_cancel() {
        let (events, next) = winit_ime_to_composition(&Ime::Disabled, true);
        assert_eq!(events, vec![CompositionEvent::Cancel]);
        assert!(!next, "Disabled closes the session");
    }

    #[test]
    fn r56_2_a_disabled_while_idle_is_idempotent_no_op() {
        let (events, next) = winit_ime_to_composition(&Ime::Disabled, false);
        assert!(events.is_empty());
        assert!(!next);
    }

    #[test]
    fn r56_2_a_full_pinyin_sequence_round_trips() {
        // Canonical pinyin "啊不" commit sequence (winit docs example).
        let mut state = false;
        let mut collected = Vec::new();
        for ime in [
            Ime::Preedit("a".to_owned(), Some((1, 1))),
            Ime::Preedit("a b".to_owned(), Some((3, 3))),
            Ime::Preedit("a b".to_owned(), Some((1, 1))),
            Ime::Preedit("\u{554A}b".to_owned(), Some((3, 3))),
            Ime::Preedit(String::new(), None),
            Ime::Commit("\u{554A}\u{4E0D}".to_owned()),
        ] {
            let (events, next) = winit_ime_to_composition(&ime, state);
            collected.extend(events);
            state = next;
        }
        assert_eq!(
            collected,
            vec![
                CompositionEvent::Start,
                CompositionEvent::Update("a".to_owned()),
                CompositionEvent::Update("a b".to_owned()),
                CompositionEvent::Update("a b".to_owned()),
                CompositionEvent::Update("\u{554A}b".to_owned()),
                CompositionEvent::Update(String::new()),
                CompositionEvent::Commit("\u{554A}\u{4E0D}".to_owned()),
            ],
        );
        assert!(!state, "session closed after Commit");
    }

    #[test]
    fn r56_2_a_full_cancel_sequence_round_trips() {
        // User escapes mid-composition: IME sends Preedit("",None)
        // then Disabled (or just Preedit("",None) if the IME stays
        // active for the next character). The Update("") clears the
        // visual; the explicit Disabled then Cancel cleans up.
        let mut state = false;
        let mut collected = Vec::new();
        for ime in [
            Ime::Preedit("\u{1112}\u{1161}".to_owned(), Some((6, 6))),
            Ime::Preedit(String::new(), None),
            Ime::Disabled,
        ] {
            let (events, next) = winit_ime_to_composition(&ime, state);
            collected.extend(events);
            state = next;
        }
        assert_eq!(
            collected,
            vec![
                CompositionEvent::Start,
                CompositionEvent::Update("\u{1112}\u{1161}".to_owned()),
                CompositionEvent::Update(String::new()),
                CompositionEvent::Cancel,
            ],
        );
        assert!(!state);
    }
}

#[cfg(test)]
mod r1009_named_key_str_tests {
    //! R1009 §5.13 — `named_key_str` content-surface vocabulary regression.
    //! The winit `NamedKey` → W3C `KeyboardEvent.key` bridge is a pure table
    //! (no `EventLoop` needed, the `winit_ime_to_composition` precedent), so the
    //! editing + function keys a terminal forwards to its PTY are pinned here
    //! directly — the winit-path half the RPC `scene/key` plane bypasses.

    use super::named_key_str;
    use winit::keyboard::NamedKey;

    #[test]
    fn editing_keys_surface_their_w3c_names() {
        // R1009 — the forcing case: a content-surface widget (sprag's PTY pane)
        // must receive these; before R1009 they were dropped at the shell.
        assert_eq!(named_key_str(NamedKey::Backspace), Some("Backspace"));
        assert_eq!(named_key_str(NamedKey::Delete), Some("Delete"));
        assert_eq!(named_key_str(NamedKey::Insert), Some("Insert"));
    }

    #[test]
    fn function_row_f1_to_f12_surfaces() {
        let row = [
            (NamedKey::F1, "F1"),
            (NamedKey::F2, "F2"),
            (NamedKey::F3, "F3"),
            (NamedKey::F4, "F4"),
            (NamedKey::F5, "F5"),
            (NamedKey::F6, "F6"),
            (NamedKey::F7, "F7"),
            (NamedKey::F8, "F8"),
            (NamedKey::F9, "F9"),
            (NamedKey::F10, "F10"),
            (NamedKey::F11, "F11"),
            (NamedKey::F12, "F12"),
        ];
        for (key, name) in row {
            assert_eq!(named_key_str(key), Some(name), "{name} surfaces");
        }
    }

    #[test]
    fn navigation_and_activation_baseline_unchanged() {
        // The R51.37 baseline is byte-identical for existing widgets.
        assert_eq!(named_key_str(NamedKey::ArrowDown), Some("ArrowDown"));
        assert_eq!(named_key_str(NamedKey::Home), Some("Home"));
        assert_eq!(named_key_str(NamedKey::PageUp), Some("PageUp"));
        assert_eq!(named_key_str(NamedKey::Enter), Some("Enter"));
        assert_eq!(named_key_str(NamedKey::Space), Some("Space"));
    }

    #[test]
    fn escape_and_tab_stay_none_filtered_upstream() {
        // Escape / Tab are offered to the focused widget by the dedicated arms
        // in handle_key_press, so the CONTENT / chord bridge deliberately does
        // NOT surface them. R1073.1: the separate DISPATCH-gate vocabulary
        // (`dispatch_named_key_str`) DOES cover them — see
        // `dispatch_gate_vocabulary_adds_escape_tab` below.
        assert_eq!(named_key_str(NamedKey::Escape), None);
        assert_eq!(named_key_str(NamedKey::Tab), None);
    }

    #[test]
    fn device_control_keys_stay_none() {
        // The curation boundary: media / browser / launch keys have no widget
        // meaning, so they stay unsurfaced (the doc's premise, correct here).
        assert_eq!(named_key_str(NamedKey::MediaPlayPause), None);
        assert_eq!(named_key_str(NamedKey::BrowserBack), None);
        assert_eq!(named_key_str(NamedKey::AudioVolumeUp), None);
    }
}

#[cfg(test)]
mod r1073_dispatch_gate_vocabulary_tests {
    //! R1073.1 PR-27.4 §5.39 — `dispatch_named_key_str` is the press-owner
    //! gate's key vocabulary: a SUPERSET of the content/chord `named_key_str`
    //! that additionally covers the two shell-reserved keys `handle_key_press`
    //! dispatches (`Escape` → exit / modal-cancel, `Tab` → focus traverse), so
    //! the close-during-dispatch gate sees them — without widening the R1009
    //! content / RPC chord vocabulary. Pure table, no `EventLoop` needed.

    use super::{dispatch_named_key_str, named_key_str};
    use winit::keyboard::NamedKey;

    #[test]
    fn dispatch_gate_vocabulary_adds_escape_tab() {
        // The whole point of R1073.1: the gate vocabulary covers the reserved
        // keys the content vocabulary excludes, so a window-closing Escape gets
        // the same one-press-one-action gating as Enter.
        assert_eq!(dispatch_named_key_str(NamedKey::Escape), Some("Escape"));
        assert_eq!(dispatch_named_key_str(NamedKey::Tab), Some("Tab"));
        assert_eq!(
            named_key_str(NamedKey::Escape),
            None,
            "content vocab still excludes them"
        );
        assert_eq!(named_key_str(NamedKey::Tab), None);
    }

    #[test]
    fn dispatch_gate_vocabulary_is_a_superset_of_named_key_str() {
        // Every key the content vocabulary names, the gate vocabulary names
        // identically — the gate only ADDS, never diverges, on the shared keys.
        for key in [
            NamedKey::Enter,
            NamedKey::Space,
            NamedKey::ArrowLeft,
            NamedKey::Backspace,
            NamedKey::F5,
            NamedKey::PageDown,
        ] {
            assert_eq!(
                dispatch_named_key_str(key),
                named_key_str(key),
                "{key:?} shared"
            );
        }
    }

    #[test]
    fn non_dispatched_keys_stay_none_in_the_gate_vocabulary() {
        // Keys the shell does not dispatch are absent from the gate too, so the
        // `None` branch in `handle_keyboard_input` only ever covers keys that
        // would not dispatch anyway (gate result moot).
        assert_eq!(dispatch_named_key_str(NamedKey::MediaPlayPause), None);
        assert_eq!(dispatch_named_key_str(NamedKey::BrowserBack), None);
    }
}

/// Run the visual binary end-to-end: build the winit event loop with
/// the [`AppEvent`] user-event slot, spawn the stdin RPC reader, run
/// the [`AppShell<V>`] until quit. The single line every shell
/// consumer needs in `fn main()`.
///
/// R51.159 §5.23 — no [`CommandExecutor`] is installed by this entry
/// point; pending [`pinion_core::Command`] queues stay parked on the
/// owner side and never fire. Use [`run_with_handlers`] to register
/// async [`Handler`](pinion_runtime::Handler)s and bind a tokio
/// runtime + intent-arrival event channel.
///
/// # Panics
/// Panics if `winit::event_loop::EventLoop::with_user_event().build()`
/// fails — that constructor only errors on platforms that cannot
/// supply a user-event loop (none of the desktop / mobile targets
/// pinion supports), so this is treated as an unrecoverable setup
/// fault rather than a propagated error.
pub fn run<V: WidgetView>() {
    // R637 §5.16 §5.7 — `PINION_SCREENSHOT=<path>` env hook. When
    // set, the binary bypasses winit entirely: build the initial
    // paint scene through the same `ShellCore` substrate the live
    // path uses, render it through `HeadlessScreenshot` (wgpu +
    // vello, no surface), write the PNG, exit cleanly. See
    // [`crate::headless_screenshot`] for the substrate rationale.
    if try_headless_screenshot::<V>() {
        return;
    }
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("winit EventLoop::with_user_event failed");
    event_loop.set_control_flow(ControlFlow::Wait);
    spawn_stdin_rpc_reader(event_loop.create_proxy());
    let mut app = AppShell::<V>::new(event_loop.create_proxy());
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("shell: event loop error: {e}");
    }
}

/// R51.159 §5.23 — variant of [`run`] that installs a
/// [`CommandExecutor`](pinion_runtime::CommandExecutor) at boot so
/// pending [`pinion_core::Command`]s queued by reducer fallout or
/// SCXML / Update steps reach their registered
/// [`Handler`](pinion_runtime::Handler)s asynchronously.
///
/// Composes:
///
/// - A tokio multi-thread [`TokioExecutor`](crate::TokioExecutor) (1
///   worker thread, `enable_all`) backing
///   [`Executor::spawn`](pinion_runtime::Executor).
/// - A [`ProxyIntentSink`](crate::ProxyIntentSink) wrapping the
///   winit [`EventLoopProxy`] so resolved [`pinion_core::Intent`]s
///   arrive on the UI thread through
///   [`AppEvent::IntentArrived`] for re-feed.
/// - The supplied `registry` of [`Handler`](pinion_runtime::Handler)
///   impls keyed by [`pinion_core::Command::kind_str`].
///
/// # Panics
/// Panics if the winit event loop cannot be built (same condition as
/// [`run`]) or if the tokio runtime cannot spin up its worker
/// thread (the OS-level thread-spawn failure that
/// [`TokioExecutor::new`](crate::TokioExecutor) wraps).
pub fn run_with_handlers<V: WidgetView>(registry: HandlerRegistry) {
    // R637 §5.16 §5.7 — see `run::<V>` for the headless screenshot
    // env contract; the handler-installing variant respects the
    // same hook so design-parity verification works for command-
    // driven examples too. Handlers are not invoked during the
    // screenshot path — the substrate captures the initial paint
    // scene only, no async resolution cycle runs.
    if try_headless_screenshot::<V>() {
        let _ = registry;
        return;
    }
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("winit EventLoop::with_user_event failed");
    event_loop.set_control_flow(ControlFlow::Wait);
    spawn_stdin_rpc_reader(event_loop.create_proxy());

    // R51.159 §5.23 — assemble the CommandExecutor and inject it
    // before the event loop starts so the first dispatch tail can
    // already drain pending commands.
    let (executor, sink) =
        build_executor_and_sink(event_loop.create_proxy()).expect("tokio runtime build failed");
    let cmd_exec = Arc::new(CommandExecutor::new(registry, executor, sink));

    let mut app = AppShell::<V>::new(event_loop.create_proxy());
    let _prior = app.core.set_command_executor(cmd_exec);

    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("shell: event loop error: {e}");
    }
}

/// R835 §5.16 — `true` when `PINION_HIDDEN_WINDOW` requests the
/// offscreen (unmapped) window mode for headless local verification (any
/// value except empty / `0`). The window still renders to its GPU
/// surface; only the OS map is suppressed, so no window flashes on the
/// developer's display while RPC demos drive the binary.
fn hidden_window_requested() -> bool {
    std::env::var("PINION_HIDDEN_WINDOW")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// R637 §5.16 §5.7 — env-hook plumbing for [`run`] / [`run_with_handlers`].
///
/// Reads `PINION_SCREENSHOT`. When unset, returns `false` so the
/// caller continues into the winit event loop. When set, builds the
/// initial paint scene through [`ShellCore::compute_paint_scene`]
/// (the same path the live render loop drives every redraw — closes
/// the [[ai-first-rpc-introspection-obligation]] gap that
/// design-parity verification cannot use the live binary in
/// headless / CI environments), renders it through
/// [`crate::headless_screenshot::HeadlessScreenshot`] (wgpu + vello,
/// no winit surface), writes a PNG, and returns `true`. Any error
/// surfaces as `eprintln!` + `std::process::exit(1)` so CI / shell
/// pipelines see a non-zero exit code on capture failure rather
/// than a silently-empty PNG.
fn try_headless_screenshot<V: WidgetView>() -> bool {
    let Ok(path) = std::env::var("PINION_SCREENSHOT") else {
        return false;
    };
    let mut core = ShellCore::<V>::new();
    // R668 §5.16 — `IntrinsicAfterFirstPaint` headless path mirrors
    // the live shell: render once at the upper bound (so the layout
    // pass sees enough viewport to lay out the content), walk for
    // the intrinsic bbox, clamp to `[min, max]`, then re-paint at
    // the clamped size so the PNG dimensions match the final winit
    // window size. `Fixed` skips the two-pass and paints directly.
    let strategy = V::initial_size_strategy();
    let (w, h, paint_scene) = match strategy {
        crate::SizeStrategy::Fixed { width, height } => {
            let scene = core.compute_paint_scene(width, height);
            (width, height, scene)
        }
        // R1059 — `OpenResizable` paints once at its open `size`, the
        // same single-pass path as `Fixed`; the OS-resize floor
        // (`min`) is a live-winit concern with no headless effect.
        crate::SizeStrategy::OpenResizable { size, .. } => {
            let scene = core.compute_paint_scene(size.0, size.1);
            (size.0, size.1, scene)
        }
        crate::SizeStrategy::IntrinsicAfterFirstPaint { min, max } => {
            let measure = core.compute_paint_scene(max.0.max(1), max.1.max(1));
            let (cw, ch) = measure.intrinsic_content_size();
            let target_w = cw.clamp(min.0, max.0).max(1);
            let target_h = ch.clamp(min.1, max.1).max(1);
            let scene = core.compute_paint_scene(target_w, target_h);
            (target_w, target_h, scene)
        }
    };
    let base = paint_adapter::root_background(&paint_scene);
    let mut vello_scene = VelloScene::new();
    // R706 §5.16 — rasterize through `to_vello_cached`, the SAME path the
    // live winit render loop drives (`AppShell::render_window`), so the
    // headless screenshot the AI introspects is pixel-faithful to the
    // live window. The previous `to_vello` (uncached) call rasterized
    // through a different code path than the live render, so a
    // cache-path-only rasterization defect (R706: the focus-ring overlay
    // drawing one grid column off through `to_vello_cached`) was visible
    // on screen yet ABSENT from the headless screenshot — defeating the
    // introspection-parity this hook exists to provide
    // ([[introspection-from-paint-not-screen]]). A fresh per-capture
    // `FragmentCache` makes every subtree a first-paint miss; the output
    // matches `to_vello` when both are correct and tracks `to_vello_cached`
    // when they would diverge.
    let mut fragment_cache = paint_adapter::FragmentCache::new();
    let mut image_cache = image_cache::ImageCache::new();
    // R1072 §5.37 — same engine-aware cached paint as the live winit path, so a
    // headless `PINION_SCREENSHOT` is pixel-faithful to the window painted via §5.37.
    let (text_cache, text_engine) = core.text_cache_and_engine();
    paint_adapter::to_vello_cached_with_text_engine(
        &paint_scene,
        &|_b: &BoxNode| None,
        text_cache,
        &mut image_cache,
        &mut fragment_cache,
        text_engine,
        &mut vello_scene,
    );
    let mut shot = match crate::HeadlessScreenshot::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("shell: PINION_SCREENSHOT: {e}");
            std::process::exit(1);
        }
    };
    let file = match std::fs::File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("shell: PINION_SCREENSHOT create {path}: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = shot.render_to_png(&vello_scene, w, h, base, std::io::BufWriter::new(file)) {
        eprintln!("shell: PINION_SCREENSHOT render_to_png: {e}");
        std::process::exit(1);
    }
    eprintln!("shell: PINION_SCREENSHOT wrote {w}x{h} RGBA8 → {path}");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;

    // ─────────────────────────────────────────────────────────────
    // R51.192 §5.45 R55.C.2 — winit ↔ W3C wheel sign convention.
    // winit's `MouseScrollDelta` positive = content moves toward
    // origin (reveal above/left); W3C `WheelEvent.deltaY` positive
    // = scroll toward content end (reveal below/right). Boundary
    // flips both axes so the substrate's `ScrollState::scroll_by`
    // receives W3C-signed deltas — matching the TUI sibling
    // (`MouseEventKind::ScrollDown` already emits `dy = +1.0`).
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r51_192_line_delta_y_flips_sign() {
        // User scrolls wheel forward (away from them) — winit
        // reports y > 0. W3C / substrate convention: forward wheel
        // moves toward content origin (deltaY < 0).
        let pinion = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(0.0, 1.0), 1.0);
        match pinion {
            WheelDelta::Lines { dx, dy } => {
                assert!((dx - 0.0).abs() < f32::EPSILON);
                assert!(
                    (dy - (-1.0)).abs() < f32::EPSILON,
                    "forward wheel must emit W3C deltaY < 0, got {dy}",
                );
            }
            other => panic!("expected Lines, got {other:?}"),
        }
    }

    #[test]
    fn r51_192_line_delta_x_flips_sign() {
        // Horizontal tilt right — winit x > 0. W3C: deltaX < 0
        // (reveals content to the left).
        let pinion = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(1.0, 0.0), 1.0);
        match pinion {
            WheelDelta::Lines { dx, dy } => {
                assert!(
                    (dx - (-1.0)).abs() < f32::EPSILON,
                    "tilt right must emit W3C deltaX < 0, got {dx}",
                );
                assert!((dy - 0.0).abs() < f32::EPSILON);
            }
            other => panic!("expected Lines, got {other:?}"),
        }
    }

    #[test]
    fn r51_192_pixel_delta_both_axes_flip() {
        // Trackpad inertia — both axes flip. The conversion narrows
        // f64 → f32 at the same boundary.
        let pinion = winit_wheel_to_pinion(
            MouseScrollDelta::PixelDelta(PhysicalPosition { x: 12.5, y: 24.0 }),
            1.0,
        );
        match pinion {
            WheelDelta::Pixels { dx, dy } => {
                assert!((dx - (-12.5)).abs() < f32::EPSILON);
                assert!((dy - (-24.0)).abs() < f32::EPSILON);
            }
            other => panic!("expected Pixels, got {other:?}"),
        }
    }

    #[test]
    fn r1027_pixel_delta_scaled_to_logical() {
        // R1027 §5.16 §5.45 — a `PixelDelta` is physical device pixels; on
        // a 2x display it must be halved to logical so the content scrolls
        // the intended distance (not 2x). The sign flip still applies.
        let pinion = winit_wheel_to_pinion(
            MouseScrollDelta::PixelDelta(PhysicalPosition { x: 12.5, y: 24.0 }),
            2.0,
        );
        match pinion {
            WheelDelta::Pixels { dx, dy } => {
                assert!(
                    (dx - (-6.25)).abs() < f32::EPSILON,
                    "12.5 physical / 2 = 6.25 logical"
                );
                assert!(
                    (dy - (-12.0)).abs() < f32::EPSILON,
                    "24 physical / 2 = 12 logical"
                );
            }
            other => panic!("expected Pixels, got {other:?}"),
        }
        // LineDelta (notch counts) is unitless — scale must NOT change it.
        let lines = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(0.0, 1.0), 2.0);
        match lines {
            WheelDelta::Lines { dy, .. } => {
                assert!(
                    (dy - (-1.0)).abs() < f32::EPSILON,
                    "line notches are scale-independent"
                );
            }
            other => panic!("expected Lines, got {other:?}"),
        }
    }

    #[test]
    fn r51_192_winit_tui_sibling_direction_agreement() {
        // Sanity guard against the conversion drifting back to a
        // pass-through: winit's forward wheel (y = +1.0) must
        // produce the same dy sign the TUI `ScrollUp` arm sends
        // (`WheelDelta::Lines { dx: 0.0, dy: -1.0 }`). If this
        // trips, the two backends disagree on direction and
        // `ScrollState::scroll_by` will move the offset opposite
        // ways per backend — §2 #6 GUI/TUI dual invariant break.
        let from_winit_forward = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(0.0, 1.0), 1.0);
        let from_tui_scroll_up = WheelDelta::Lines { dx: 0.0, dy: -1.0 };
        match (from_winit_forward, from_tui_scroll_up) {
            (WheelDelta::Lines { dy: w_dy, .. }, WheelDelta::Lines { dy: t_dy, .. }) => {
                assert!(
                    w_dy.signum() == t_dy.signum(),
                    "winit forward must match TUI ScrollUp sign (both negative dy); \
                     got winit={w_dy} vs tui={t_dy}",
                );
            }
            _ => panic!("both branches must be Lines variants"),
        }
    }

    // ─────────────────────────────────────────────────────────────
    // R1027 §5.16 §5.35 — `HiDPI` scale_factor coordinate policy. The
    // whole pinion scene / layout / pointer world is logical; physical
    // pixels appear only at the GPU surface, the vello raster scale,
    // and these winit input + layout-dim boundaries. These pin the pure
    // conversions (the shell wires them into render_window + the
    // CursorMoved / Touch arms).
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r1027_logical_layout_size_divides_physical_by_scale() {
        // 1920x1200 physical on a 2x display = 960x600 logical — the
        // size the paint scene is built in (app dims rastered at 2x).
        assert_eq!(
            logical_layout_size(PhysicalSize::new(1920, 1200), 2.0),
            (960, 600)
        );
        // Identity: logical == physical (non-`HiDPI`, unchanged path).
        assert_eq!(
            logical_layout_size(PhysicalSize::new(800, 600), 1.0),
            (800, 600)
        );
        // Fractional scale rounds to nearest: 1440/1.5=960, 901/1.5=600.67->601.
        assert_eq!(
            logical_layout_size(PhysicalSize::new(1440, 901), 1.5),
            (960, 601)
        );
        // Degenerate: a sub-logical-pixel physical dimension (1px / 4 =
        // 0.25 -> round -> 0) returns 0 — NOT clamped to 1. render_window's
        // `NonZeroU32` guard then early-returns (no paint), matching the
        // pre-R1027 0-size skip rather than painting a wasted 1px frame.
        assert_eq!(logical_layout_size(PhysicalSize::new(1, 1), 4.0), (0, 0));
    }

    #[test]
    fn r1027_pointer_physical_to_logical() {
        // The splitter case (PR-15): a physical cursor at x=960 on a 2x
        // display is logical 480 — exactly the 4-logical-px handle the
        // router must resolve. Pre-R1027 it saw 960 and missed the handle.
        assert_eq!(
            winit_pointer_to_logical(PhysicalPosition::new(960.0, 600.0), 2.0),
            (480.0, 300.0)
        );
        // Identity is a pass-through (byte-identical pre-R1027 routing).
        assert_eq!(
            winit_pointer_to_logical(PhysicalPosition::new(12.0, 34.0), 1.0),
            (12.0, 34.0)
        );
        // Fractional scale: 150/1.5 = 100, 75/1.5 = 50.
        let (x, y) = winit_pointer_to_logical(PhysicalPosition::new(150.0, 75.0), 1.5);
        assert!((x - 100.0).abs() < f64::EPSILON);
        assert!((y - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn r1027_scale_is_non_identity_gate() {
        // Drives the byte-identical fast path (scaled append + AccessKit
        // root transform are both skipped at identity).
        assert!(!scale_is_non_identity(1.0));
        assert!(scale_is_non_identity(2.0));
        assert!(scale_is_non_identity(1.5));
        // A downscaled (< 1.0) display is also non-identity.
        assert!(scale_is_non_identity(0.75));
    }
}

#[cfg(test)]
mod r1121_chrome_action_tests {
    //! R1121 §5.16 §5.39 — the window-chrome tag -> control mapping that
    //! `try_chrome_press` routes to winit. Pure, so the full vocabulary is
    //! covered without a live window.
    use super::{ChromeAction, chrome_action_for_tag};
    use winit::window::ResizeDirection;

    #[test]
    fn maps_every_chrome_control_tag() {
        assert!(matches!(
            chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_CLOSE_TAG),
            Some(ChromeAction::Close)
        ));
        assert!(matches!(
            chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_MINIMIZE_TAG),
            Some(ChromeAction::Minimize)
        ));
        assert!(matches!(
            chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_MAXIMIZE_TAG),
            Some(ChromeAction::Maximize)
        ));
        assert!(matches!(
            chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_GRIP_TAG),
            Some(ChromeAction::Move)
        ));
    }

    #[test]
    fn maps_every_resize_region_tag_to_its_direction() {
        // R1122 — the eight resize edges / corners map to the matching winit
        // `ResizeDirection` that `try_chrome_press` feeds `drag_resize_window`.
        let cases = [
            (
                pinion_overlay::WINDOW_RESIZE_NORTH_TAG,
                ResizeDirection::North,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_SOUTH_TAG,
                ResizeDirection::South,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_WEST_TAG,
                ResizeDirection::West,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_EAST_TAG,
                ResizeDirection::East,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_NORTH_WEST_TAG,
                ResizeDirection::NorthWest,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG,
                ResizeDirection::NorthEast,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_SOUTH_WEST_TAG,
                ResizeDirection::SouthWest,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_SOUTH_EAST_TAG,
                ResizeDirection::SouthEast,
            ),
        ];
        for (tag, dir) in cases {
            assert!(
                matches!(chrome_action_for_tag(tag), Some(ChromeAction::Resize(d)) if d == dir),
                "{tag} maps to ResizeDirection::{dir:?}",
            );
        }
    }

    #[test]
    fn non_chrome_tags_are_not_controls() {
        assert!(chrome_action_for_tag("some-widget").is_none());
        assert!(chrome_action_for_tag("ai-overlay/focus-ring").is_none());
        // The strip CONTAINER tag is not itself a control (its children are).
        assert!(chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_TAG).is_none());
        // The resize family PREFIX is not itself a control (the suffixed
        // edge / corner tags are).
        assert!(chrome_action_for_tag(pinion_overlay::WINDOW_RESIZE_TAG_PREFIX).is_none());
    }
}
