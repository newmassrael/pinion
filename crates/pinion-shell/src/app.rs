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
use pinion_runtime::{paint_adapter, CommandExecutor, HandlerRegistry, PointerId};
use vello::Scene as VelloScene;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::executor::build_executor_and_sink;
use crate::substrate::ShellCore;
use crate::{AppEvent, RenderState, SizeStrategy, VelloContext, VelloRenderer, WidgetRenderer, WidgetView, WindowSpec};

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
    /// R671 §5.12 §5.16 — per-window last-painted [`LayoutNode`]
    /// snapshot. R670.B's single `ShellCore.last_paint_layout` was
    /// binding-wide (the last window to paint wrote into it), which
    /// made `scene/layout {viewport: null, window: "<id>"}` return
    /// whichever window painted most recently rather than the
    /// addressed window. R671 lifts the snapshot per-slot so each
    /// window keeps its own last-painted layout independently;
    /// `AppShell::render_window` writes here after each paint cycle,
    /// and `AppShell::dispatch_rpc` reads here when resolving the
    /// `{window: "<id>"}` JSON-RPC param. The substrate's
    /// `ShellCore.last_paint_layout` is kept as the primary mirror
    /// for backward-compatible single-window callers
    /// (`ShellCore::dispatch_rpc` without a window scope).
    last_paint_layout: Option<pinion_rpc::LayoutNode>,
    /// R681 §2 #4 atomic 2 — sticky flag set whenever the most recent
    /// per-window paint scene contained at least one
    /// [`pinion_core::Scene::ImmediateModeNode`]. The
    /// [`ApplicationHandler::about_to_wait`] override consults this
    /// flag to pick between [`ControlFlow::Wait`] (input-driven, idle
    /// power) and [`ControlFlow::WaitUntil`] (per-window paint clock
    /// at ~60 fps) — the §2 #4 game-loop contract for the
    /// immediate-mode subtree opt-in. Cleared automatically each
    /// paint cycle: if the view fn stops emitting an immediate-mode
    /// subtree, the flag falls back to `false` on the next paint and
    /// the slot returns to input-driven pacing.
    has_immediate_mode_subtree: bool,
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
}

impl<R: VelloRenderer> WindowSlot<R> {
    /// R682 §5.16 — collapse the per-slot field defaults that the
    /// `resume_spec` code path repeats at both the suspended-init and
    /// active-init sites. Only `render`, `accesskit`, `spec_id`, and
    /// `pending_intrinsic_resize` vary between the two sites; every
    /// other field starts at its canonical empty-state value.
    fn build(
        render: RenderState<R>,
        accesskit: Option<accesskit_winit::Adapter>,
        spec_id: Cow<'static, str>,
        pending_intrinsic_resize: Option<((u32, u32), (u32, u32))>,
    ) -> Self {
        Self {
            render,
            vello_scene: VelloScene::new(),
            accesskit,
            ime_was_composing: false,
            last_ime_cursor_area: None,
            pending_intrinsic_resize,
            spec_id,
            last_paint_layout: None,
            has_immediate_mode_subtree: false,
            fragment_cache: paint_adapter::FragmentCache::new(),
        }
    }
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
        Self {
            core: ShellCore::new(),
            windows: HashMap::new(),
            spec_id_to_window_id: HashMap::new(),
            primary_window_id: None,
            proxy,
            windows_signal: None,
            reconcile_effect: None,
            last_known_specs: Rc::new(RefCell::new(Vec::new())),
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
        let window_id_choice = parsed_request
            .params
            .as_ref()
            .and_then(|p| p.get("window"))
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| String::from("main"), str::to_owned);
        let resolved_spec_id = self.resolve_spec_id(&window_id_choice);
        // R670.B §5.16 — primary-window-scoped `scene/resize` (the
        // resize closure still targets the primary window; per-window
        // `scene/resize` is a follow-up axis once a real consumer
        // surfaces — typical multi-window app resizes the main
        // window, not the inspector). Holding the Arc<Window> across
        // the substrate call keeps the closure's `&Arc<Window>`
        // borrow alive without re-borrowing `self.windows` from
        // inside the closure.
        let primary_window_arc: Option<Arc<Window>> = self
            .primary_slot()
            .and_then(Self::slot_window)
            .cloned();
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
        // R671 §5.12 — resolve the addressed window's
        // [`WindowSlot::last_paint_layout`] so the substrate's
        // dispatcher answers `scene/layout {viewport: null}` against
        // the *named* window's last paint (not the binding-wide
        // primary mirror). The lookup walks `spec_id_to_window_id`
        // first (resolved_spec_id is a canonical spec id; the slot
        // map is keyed by winit `WindowId`); fallback `None` leaves
        // the substrate reading its primary mirror exactly as
        // pre-R671 / single-window callers.
        let slot_layout_owned: Option<pinion_rpc::LayoutNode> = self
            .spec_id_to_window_id
            .iter()
            .find(|(id, _)| **id == resolved_spec_id.as_str())
            .and_then(|(_, win_id)| self.windows.get(win_id))
            .and_then(|slot| slot.last_paint_layout.clone());
        let resp = self.core.dispatch_rpc_for_window(
            parsed_request,
            &resolved_spec_id,
            slot_layout_owned.as_ref(),
            &mut resize_req,
        );
        if let Some(resp) = resp {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{resp}").is_err() {
                // stdout closed (downstream consumer gone) — silently
                // skip; do not abort the GUI loop on a broken pipe.
            }
        }
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
        paint_scene: &pinion_core::Scene,
        size_w: u32,
        size_h: u32,
    ) {
        let Some(slot) = self.windows.get_mut(&window_id) else { return };
        if slot.accesskit.is_none() {
            return;
        }
        let (nodes, at_focus) = self.core.collect_access_emit_inputs(paint_scene);
        let window_bounds = pinion_core::scene::Rect::new(0, 0, size_w, size_h);
        let decision = self.core.plan_access_emit(&nodes, at_focus.as_ref());
        // Re-acquire the slot mutable borrow now that the substrate
        // borrows released (collect_access_emit_inputs +
        // plan_access_emit take `&mut self.core`).
        let Some(slot) = self.windows.get_mut(&window_id) else { return };
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
                builder.build(Some(window_bounds))
            });
        }
        // R51.77 / R51.79 §5.40 — commit step. By-value Vec move
        // into the cache; nodes consumed here. Idempotent on
        // !should_emit so the next frame's plan diffs against the
        // post-emit baseline.
        self.core.commit_access_emit(nodes, at_focus.as_ref());
    }

    /// R670.B §5.16 — resolve the supplied window id to a known
    /// spec id. Falls back to the primary spec id when the supplied
    /// id is missing or unknown (AI clients targeting a window that
    /// doesn't exist see the primary's scene rather than a hard
    /// error — single-window bindings always have a primary; multi-
    /// window bindings can detect the fallback by comparing the
    /// returned id against the supplied id).
    ///
    /// Returns an owned `String` so the borrow on `self.windows`
    /// releases before the caller threads the id into the substrate's
    /// producer closure (which takes its own `&mut self.core`).
    fn resolve_spec_id(&self, supplied: &str) -> String {
        // R683 §5.16 — `spec_id_to_window_id` is keyed by
        // `Cow<'static, str>`; `HashMap::contains_key` takes
        // `&Q where K: Borrow<Q>` and `Cow<'static, str>:
        // Borrow<str>`, so the plain `&str` lookup still works.
        if self.spec_id_to_window_id.contains_key(supplied) {
            return supplied.to_string();
        }
        // Fall back to the primary spec id (the first spec, by
        // construction always present in `windows` after `resumed`).
        match self.primary_slot() {
            Some(slot) => slot.spec_id.to_string(),
            None => "main".to_string(),
        }
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
        let (spec_id, w, h) = {
            let Some(slot) = self.windows.get(&window_id) else {
                return;
            };
            let RenderState::Active { window, .. } = &slot.render else {
                return;
            };
            let size = window.inner_size();
            let Some(w) = core::num::NonZeroU32::new(size.width) else { return };
            let Some(h) = core::num::NonZeroU32::new(size.height) else { return };
            (slot.spec_id.clone(), w, h)
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
        let paint_scene = self
            .core
            .compute_paint_scene_for_window(&spec_id, w.get(), h.get());
        // Re-acquire the slot mutable borrow now that the substrate
        // borrow released, then bind window + renderer for the
        // intrinsic-resize hook + vello submit. Scope the borrow
        // so it drops before the post-paint helpers
        // (emit_accesskit_for_window, publish_ime_for_window) which
        // take `&mut self`.
        let size = {
            let Some(slot) = self.windows.get_mut(&window_id) else { return };
            let RenderState::Active { window, renderer } = &mut slot.render else {
                return;
            };
            let size = window.inner_size();
        // R668 §5.16 — `IntrinsicAfterFirstPaint` post-first-paint
        // resize hook (per-window since R670.B). The first painted
        // scene now carries layout-computed rects on every node, so
        // walking the tree ([`Scene::intrinsic_content_size`]) gives
        // us the tight (width, height) the content actually wants;
        // clamp to `[min, max]` and forward to
        // `Window::request_inner_size`. winit emits a
        // `WindowEvent::Resized` on acceptance which re-enters the
        // layout pass at the new viewport on the next paint. The
        // hook drains itself — `Fixed`-strategy paints and every
        // steady-state paint after the first land on the `None`
        // branch and skip out.
        if let Some((min, max)) = slot.pending_intrinsic_resize.take() {
            let (content_w, content_h) = paint_scene.intrinsic_content_size();
            let target_w = content_w.clamp(min.0, max.0);
            let target_h = content_h.clamp(min.1, max.1);
            if (target_w, target_h) != (w.get(), h.get()) {
                let _ = window.request_inner_size(LogicalSize::new(
                    f64::from(target_w),
                    f64::from(target_h),
                ));
                // Force-request a redraw so the next event-loop pass
                // re-enters `render` against the updated inner_size
                // and paints the final layout immediately rather than
                // idling on the now-undersized first-paint frame.
                window.request_redraw();
            }
        }
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
        paint_adapter::to_vello_cached(
            &paint_scene,
            &|_b: &BoxNode| None,
            self.core.text_cache_mut(),
            &mut slot.fragment_cache,
            &mut slot.vello_scene,
        );
        // R51.109.1 §5.41 — call through the backend-agnostic
        // `WidgetRenderer` trait. `VelloContext::base_color` carries
        // the window background sampled from
        // `paint_adapter::root_background`; the renderer's macro impl
        // forwards to the inherent `<R>::render(frame, base_color)`.
        // `renderer.render` auto-derefs through `Box<R>` because the
        // `WidgetRenderer` trait is in scope.
        if let Err(e) = renderer.render(&slot.vello_scene, VelloContext { base_color: base }) {
            eprintln!("shell: vello render: {e}");
        }
            size
        };
        // The post-paint helpers (`emit_accesskit_for_window`,
        // `publish_ime_for_window`) each re-acquire their own slot
        // borrow internally and take `&mut self.core` — the scope
        // above released the long-held slot borrow so this is safe.
        self.emit_accesskit_for_window(
            window_id,
            &paint_scene,
            size.width,
            size.height,
        );
        // R56.2.c §5.13 §5.38 — push IME candidate window position
        // to the platform IME (per-window since R670.B).
        self.publish_ime_for_window(window_id, &paint_scene);
        // R671 §5.12 §5.16 — build the per-window layout snapshot
        // exactly once, before `finalize_frame` consumes the paint
        // scene. The single build feeds both the per-slot
        // `last_paint_layout` (multi-window `scene/layout {window:
        // "<id>"}` reads from here through `AppShell::dispatch_rpc`)
        // and the `ShellCore.last_paint_layout` primary mirror that
        // backs single-window `dispatch_rpc(...)` callers. The slot
        // gets a clone — `LayoutNode` is `Clone` and the cost is
        // proportional to the painted tree size (small for the
        // current widget catalog; the next big consumer is the
        // DevTools/Inspector axis which already buys this cost).
        let paint_layout = pinion_rpc::build_layout_node(&paint_scene, "/0");
        // R683 §5.16 — `spec_id` is `Cow<'static, str>`; clone for
        // the post-borrow `finalize_frame_for_window` call (clones
        // are `Cow`-cheap for `Borrowed`, `String::clone` for
        // `Owned`).
        let spec_id_for_finalize = self
            .windows
            .get(&window_id)
            .map(|s| s.spec_id.clone());
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
        if let Some(slot) = self.windows.get_mut(&window_id) {
            slot.last_paint_layout = Some(paint_layout.clone());
            // R681 §2 #4 atomic 2 — sticky per-window flag for the
            // immediate-mode game-loop pacing. The next
            // `about_to_wait` reads this to choose between
            // [`ControlFlow::Wait`] and
            // [`ControlFlow::WaitUntil(deadline)`]. The substrate
            // tick walker already armed the per-window redraw flag
            // inside `compute_paint_scene_internal`; the sticky
            // signal here lets the pacing decision survive the
            // single-shot redraw-flag drain.
            slot.has_immediate_mode_subtree =
                paint_scene.has_immediate_mode_subtree();
        }
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
        // R51.80 §5.12 §5.35 — hand the rendered scene + the pre-
        // built layout snapshot to the substrate. `finalize_frame`
        // refreshes the input router + intent drain in one method
        // and stores the layout on `ShellCore.last_paint_layout` as
        // the primary mirror.
        //
        // R672 §5.35 §5.41 — route through `finalize_frame_for_window`
        // so the addressed window's [`pinion_runtime::InputRouter`]
        // (not the binding-wide single router pre-R672) sees the
        // paint scene. Each window's pointer state stays isolated;
        // cross-window paint cycles no longer flip-flop hover state.
        self.core
            .finalize_frame_for_window(target_window, paint_scene, paint_layout);
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
    fn publish_ime_for_window(
        &mut self,
        window_id: WindowId,
        paint_scene: &pinion_core::Scene,
    ) {
        let cached_state = *self.core.cached_state();
        let focused_owned = self.core.focus().focused().map(str::to_owned);
        let owner = self.core.root_owner().clone();
        let ime_rect = owner.run(|| {
            V::ime_caret_rect(&cached_state, paint_scene, focused_owned.as_deref())
        });
        let Some(rect) = ime_rect else { return };
        let rect_tuple = (rect.x, rect.y, rect.width, rect.height);
        let Some(slot) = self.windows.get_mut(&window_id) else { return };
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
    fn handle_key_press(
        &mut self,
        event_loop: &ActiveEventLoop,
        logical_key: &Key,
    ) {
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
                let handled = self.core.try_apply_key("Escape");
                if !handled && !self.core.focus_is_modal() {
                    event_loop.exit();
                }
            }
            Key::Named(NamedKey::Tab) => {
                self.core.handle_focus_traverse(self.core.modifiers_shift_key());
            }
            Key::Character(c) => self.core.handle_character_key(c),
            Key::Named(named) => {
                if let Some(key_str) = named_key_str(named) {
                    self.core.handle_named_key(key_str);
                }
            }
            _ => {}
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
        // Update the cache so the next `reconcile_windows` call
        // diffs against the snapshot the shell just acted on.
        *self.last_known_specs.borrow_mut() = new_specs;
        // Re-request paint on every active window so the next event
        // loop iteration renders the new topology. drain dispatches
        // a Window::request_redraw per active slot.
        self.core.request_redraw();
        self.drain_redraw_to_winit();
    }

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
            w
        } else {
            let mut attrs = Window::default_attributes()
                .with_title(spec.title.clone())
                .with_inner_size(LogicalSize::new(f64::from(init_w), f64::from(init_h)));
            // R668 §5.16 — anchor the user-driven OS-resize floor at
            // `min` so dragging the resize chrome smaller than the
            // intrinsic floor stops at `min`. winit clamps the floor
            // to the OS-imposed minimum (~100×30 desktop) anyway.
            let min_floor = match strategy {
                SizeStrategy::Fixed { width, height } => (width, height),
                SizeStrategy::IntrinsicAfterFirstPaint { min, .. } => min,
            };
            attrs = attrs.with_min_inner_size(LogicalSize::new(
                f64::from(min_floor.0),
                f64::from(min_floor.1),
            ));
            match event_loop.create_window(attrs) {
                Ok(w) => Arc::new(w),
                Err(e) => {
                    eprintln!("shell: window create ({}) failed: {e}", &spec.id);
                    event_loop.exit();
                    return;
                }
            }
        };
        let pending_intrinsic_resize = match strategy {
            SizeStrategy::Fixed { .. } => None,
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
        let size = window.inner_size();
        let renderer = pollster::block_on(<V::Renderer as VelloRenderer>::new(
            Arc::clone(&window),
            size.width.max(1),
            size.height.max(1),
        ));
        let renderer = match renderer {
            Ok(r) => r,
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
        let slot = WindowSlot::build(
            RenderState::Active {
                window,
                renderer: Box::new(renderer),
            },
            Some(adapter),
            spec.id.clone(),
            pending_intrinsic_resize,
        );
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
            eprintln!(
                "shell: V::windows() returned empty list; nothing to create",
            );
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
            let cached_window_id = self
                .spec_id_to_window_id
                .get(&*spec.id)
                .copied();
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
                // R51.38 §5.35 — winit mouse events are single-source
                // on every desktop platform pinion supports; the
                // shell threads `PointerId::MOUSE` unconditionally.
                self.core.cursor_moved_for_window(
                    spec_id,
                    PointerId::MOUSE,
                    position.x,
                    position.y,
                );
            }
            WindowEvent::CursorLeft { .. } => {
                self.core.cursor_left_for_window(spec_id, PointerId::MOUSE);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.core.mouse_pressed_for_window(spec_id, PointerId::MOUSE);
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.core.mouse_released_for_window(spec_id, PointerId::MOUSE);
            }
            // R56.2.e §5.13 §5.22 — middle-mouse-button press routes
            // to `WidgetView::apply_middle_click` (the canonical
            // X11 / Wayland "paste PRIMARY at the focused text
            // widget" UX path). winit fires this arm for every
            // platform's middle-button press (winit normalises X11
            // ButtonEvent / Wayland `wl_pointer` button / macOS
            // `NSEvent` otherMouseDown / Windows `WM_MBUTTONDOWN`
            // under one enum); the substrate's `ShellCore::middle_click`
            // reads the focused tag from the focus manager and
            // dispatches through `CoreShell::apply_middle_click`,
            // which wraps the trait call in `root_owner.run`
            // (R51.152) so application impls can reach the same
            // reactive hooks the keyboard path uses.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Middle,
                ..
            } => {
                self.core.middle_click();
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
                let pinion_delta = winit_wheel_to_pinion(delta);
                self.core
                    .wheel_for_window(spec_id, PointerId::MOUSE, pinion_delta);
            }
            // R51.45 §5.35 — winit `WindowEvent::Touch` closes the
            // R51.38 multi-pointer first-design substrate arc.
            // R51.108 §5.41 — convert at the winit boundary so the
            // substrate sees only the abstract `pinion_runtime::Touch`.
            WindowEvent::Touch(touch) => {
                self.core
                    .touch_event_for_window(spec_id, winit_touch_to_pinion(touch));
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // R51.53 §5.39 — winit emits `KeyEvent` without
                // modifier state, so cache the most-recent value
                // out-of-band for Shift+Tab detection.
                // R51.108 §5.41 — convert at the winit boundary.
                self.core.set_modifiers(winit_modifiers_to_pinion(modifiers.state()));
            }
            WindowEvent::Focused(focused) => {
                // R51.59 §5.39 — Window blur / refocus. ARIA Focus
                // Order asks the framework to reinstate the focused
                // widget when the user returns to the window.
                if focused {
                    self.core.window_focused();
                } else {
                    self.core.window_blurred();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    self.handle_key_press(event_loop, &event.logical_key);
                }
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
            WindowEvent::Resized(size) => {
                // R670.B §5.16 — per-window resize. The matching slot
                // holds the live GPU renderer the wgpu surface
                // resize-event must reach.
                if let Some(slot) = self.windows.get_mut(&window_id)
                    && let RenderState::Active { renderer, .. } = &mut slot.render
                {
                    renderer.resize(size.width.max(1), size.height.max(1));
                }
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
        }
        self.drain_redraw_to_winit();
    }

    /// R681 §2 #4 atomic 2 §5.16 §5.28 — per-window game-loop pacing.
    /// winit calls `about_to_wait` after every batch of pending events
    /// has been drained and immediately before the event loop blocks
    /// for more input. This is the canonical hook to configure the
    /// next [`ControlFlow`]:
    ///
    /// - Any active window slot with `has_immediate_mode_subtree =
    ///   true` arms the §2 #4 game-loop branch: compute that slot's
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
            // R681 atomic 3 — derive the per-window frame budget
            // from the substrate signals: the slot's sticky
            // `has_immediate_mode_subtree` flag + the binding's
            // optional `set_target_fps_for_window` override. `None`
            // budget means "this slot does not contribute a
            // deadline" (idle policy, the default for retained-tree
            // windows).
            let override_fps = self.core.target_fps_for_window(&slot.spec_id);
            let budget = pinion_runtime::frame_pacing::frame_budget_for_window(
                slot.has_immediate_mode_subtree,
                override_fps,
            );
            let Some(budget) = budget else { continue };
            immediate_slot_ids.push(slot.spec_id.clone());
            let deadline = match self
                .core
                .last_paint_instant_for_window(&slot.spec_id)
            {
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


/// R51.37 §5.35 — bridge from winit's [`NamedKey`] enum to the
/// W3C-aligned `KeyboardEvent.key` strings the
/// [`WidgetView::apply_key`] contract speaks. Only the keys with
/// established cross-platform widget meanings are surfaced;
/// `NamedKey::Escape` is filtered upstream (shell-reserved quit),
/// `NamedKey::Tab` is filtered upstream (R51.53 §5.39 `FocusManager`
/// swallow), and unmapped variants return `None` so the shell stays
/// silent on keys no widget cares about. The ASCII / W3C names match
/// the strings Material / `SwiftUI` / Qt / W3C ARIA Slider authoring
/// patterns specify, so a widget implementation can match against
/// the same identifiers a browser-side application would consume.
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
        _ => None,
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
fn winit_touch_to_pinion(touch: winit::event::Touch) -> pinion_runtime::Touch {
    pinion_runtime::Touch {
        id: touch.id,
        x: touch.location.x,
        y: touch.location.y,
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
/// variants. The `PhysicalPosition<f64>` narrows to `f32` here
/// (winit's logical-pixel coordinates already use `f32` precision;
/// the substrate's `wheel_delta_to_pixels` rounds to `i32`, so the
/// wider `f64` carries no information past the boundary).
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
fn winit_wheel_to_pinion(delta: MouseScrollDelta) -> WheelDelta {
    match delta {
        MouseScrollDelta::LineDelta(dx, dy) => WheelDelta::Lines { dx: -dx, dy: -dy },
        MouseScrollDelta::PixelDelta(pos) => WheelDelta::Pixels {
            dx: -(pos.x as f32),
            dy: -(pos.y as f32),
        },
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
fn winit_theme_to_pinion_scheme(
    theme: winit::window::Theme,
) -> pinion_core::SystemColorScheme {
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
            vec![CompositionEvent::Start, CompositionEvent::Update("ha".to_owned())],
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
        let (events, next) =
            winit_ime_to_composition(&Ime::Preedit(String::new(), None), true);
        assert_eq!(events, vec![CompositionEvent::Update(String::new())]);
        assert!(next, "empty preedit during session keeps was_composing");
    }

    #[test]
    fn r56_2_a_empty_preedit_while_idle_is_idempotent_no_op() {
        let (events, next) =
            winit_ime_to_composition(&Ime::Preedit(String::new(), None), false);
        assert!(events.is_empty());
        assert!(!next);
    }

    #[test]
    fn r56_2_a_commit_during_session_dispatches_commit_and_closes_session() {
        // Pinyin / Hangul canonical sequence: …Preedit("han") →
        // Preedit("", None) [synthetic clear, dispatched as
        // Update("")] → Commit("\u{D55C}") lands here with
        // was_composing=true.
        let (events, next) =
            winit_ime_to_composition(&Ime::Commit("\u{D55C}".to_owned()), true);
        assert_eq!(events, vec![CompositionEvent::Commit("\u{D55C}".to_owned())]);
        assert!(!next, "Commit closes the session");
    }

    #[test]
    fn r56_2_a_commit_without_session_injects_synthetic_start() {
        // macOS dead-key sequences emit Commit without a prior
        // Preedit. Inject a synthetic Start so the substrate drives
        // through Focused → Editing and the `was_composing` gate
        // inside `apply_composition_commit` fires the
        // `text_committed` intent.
        let (events, next) =
            winit_ime_to_composition(&Ime::Commit("e\u{301}".to_owned()), false);
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
    let (executor, sink) = build_executor_and_sink(event_loop.create_proxy())
        .expect("tokio runtime build failed");
    let cmd_exec = Arc::new(CommandExecutor::new(registry, executor, sink));

    let mut app = AppShell::<V>::new(event_loop.create_proxy());
    let _prior = app.core.set_command_executor(cmd_exec);

    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("shell: event loop error: {e}");
    }
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
    paint_adapter::to_vello(
        &paint_scene,
        &|_b: &BoxNode| None,
        core.text_cache_mut(),
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
        let pinion = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(0.0, 1.0));
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
        let pinion = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(1.0, 0.0));
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
        let pinion = winit_wheel_to_pinion(MouseScrollDelta::PixelDelta(PhysicalPosition {
            x: 12.5,
            y: 24.0,
        }));
        match pinion {
            WheelDelta::Pixels { dx, dy } => {
                assert!((dx - (-12.5)).abs() < f32::EPSILON);
                assert!((dy - (-24.0)).abs() < f32::EPSILON);
            }
            other => panic!("expected Pixels, got {other:?}"),
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
        let from_winit_forward =
            winit_wheel_to_pinion(MouseScrollDelta::LineDelta(0.0, 1.0));
        let from_tui_scroll_up = WheelDelta::Lines { dx: 0.0, dy: -1.0 };
        match (from_winit_forward, from_tui_scroll_up) {
            (
                WheelDelta::Lines { dy: w_dy, .. },
                WheelDelta::Lines { dy: t_dy, .. },
            ) => {
                assert!(
                    w_dy.signum() == t_dy.signum(),
                    "winit forward must match TUI ScrollUp sign (both negative dy); \
                     got winit={w_dy} vs tui={t_dy}",
                );
            }
            _ => panic!("both branches must be Lines variants"),
        }
    }
}
