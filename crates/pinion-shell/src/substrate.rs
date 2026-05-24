//! R51.92 §5.40 — substrate module split from `lib.rs`.
//!
//! Houses the framework-side dispatch substrate ([`ShellCore`]) and
//! the pure planning decision struct it returns
//! ([`AccessEmitDecision`]). Pre-R51.92 these types lived in `lib.rs`
//! alongside [`crate::AppShell`]; the visibility downgrade in R51.83
//! (14 `pub(crate)` → private fields + `core: ShellCore<V>` field on
//! `AppShell`) had no type-level effect because the surface code
//! shared the same module as the substrate.
//!
//! R51.92 promotes the encapsulation from a single-file convention
//! to an enforced module boundary: every `ShellCore` field is private
//! to this module, and `AppShell` (in `crate::root`) reaches the
//! substrate only through the accessor / dispatch methods declared
//! `pub` here.
//!
//! R51.123 §5.41 — the backend-agnostic dispatch core
//! ([`pinion_runtime::CoreShell<V>`]) lives behind a single
//! `core: CoreShell<V>` field. Every dispatch method on this struct
//! now reduces to "call `core.X` → log the returned
//! [`pinion_runtime::DispatchTail`] → bump backend-specific
//! bookkeeping (OCC revision, focus, redraw flag)". The four pieces
//! the lift moved out (`scene`, `cached_state`, `router`,
//! `intent_queue`) live inside `core`; this struct keeps only the
//! Vello-specific state (focus / modifiers / `text_cache` / previews
//! / revision / `last_paint_layout` / `last_access_*` / `redraw_requested`).
//!
//! See `substrate-incompleteness-signal` (the R51.29 → R51.30
//! refactor that birthed the shell) and `claim-accuracy-self-audit`
//! (the R51.80 → R51.83 → R51.92 lesson that "wrapper added" /
//! "visibility downgraded" / "module split" are three different
//! substantive depths of the same encapsulation claim).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use accesskit::NodeId;
use pinion_a11y::{
    tag_to_node_id, translate_action, AccessAction, AccessFocus, AccessNode,
    PinionAccessAction, ROOT_NODE_ID,
};
use pinion_core::event::WheelDelta;
use pinion_core::{Frame, Intent, Scene, SceneRevision};
use pinion_rpc::{
    build_layout_node, dispatch, DeferredInput, DispatchContext, LayoutNode, PreviewLedger,
};
use pinion_runtime::{
    clamp_frame_dt, compute_layout, compute_layout_with_scroll_dirty, rect_for_tag,
    CommandExecutor, CoreShell, DispatchTail,
    FocusManager, Modifiers, PointerId, Touch, TouchPhase,
};
use pinion_text::LayoutCache;

use super::WidgetView;

/// R51.76 §5.40 — framework-side dispatch substrate, decoupled from
/// winit / wgpu / `accesskit_winit`.
///
/// [`ShellCore`] owns every piece of state that `AppShell`'s dispatch
/// path mutates: the authoritative state scene, the cached state
/// projection, the §5.20 intent queue, the §5.34 preview ledger +
/// revision token, the §5.35 input router, the §5.39 focus manager
/// and cached winit modifier state, the §5.36 text layout cache, the
/// §5.12 last-paint snapshot, and the §5.40 incremental AT-emit
/// caches (tag map / node diff / focus diff / initial-emit flag).
///
/// The split is the textbook substrate/surface separation: the
/// shell-coupled bits (winit `Window`, wgpu surface, Vello renderer,
/// `accesskit_winit::Adapter`, `EventLoopProxy`) live in
/// [`crate::AppShell`]; everything else lives here so the dispatch
/// surface is reachable from headless tests without standing up a
/// winit `EventLoop` or a real wgpu device.
///
/// `request_redraw` no longer touches a `Window` directly — it sets
/// [`ShellCore::redraw_requested`] so [`crate::AppShell`] can drain
/// the flag once per event-loop iteration and forward to
/// `Window::request_redraw` when a `Window` exists, while headless
/// tests just observe the flag.
///
/// R51.92 §5.40 — moved from `lib.rs` to its own module so every
/// field below is genuinely private to `substrate.rs` and the
/// surface module (`AppShell` in `lib.rs`) can only reach the
/// dispatch path through the `pub` methods on this `impl` block.
pub struct ShellCore<V: WidgetView> {
    /// R51.123 §5.41 — backend-agnostic dispatch substrate.
    ///
    /// Owns the four pieces of state the lift moved out of this
    /// struct: `scene`, `cached_state`, `router`, `intent_queue`.
    /// Every dispatch method on `ShellCore` reduces to "call `core.X`
    /// for the SCXML / router work, then handle the returned
    /// [`DispatchTail`] (log intents + log state change + bump
    /// `redraw_requested` on visible-state transition)".
    ///
    /// The TUI sibling (`pinion_tui::ShellCoreTui`) composes the same
    /// [`CoreShell`] inner struct so both backends share scene +
    /// router + intent-queue handling without duplicating the
    /// dispatch arms (R51.123 / R51.124 / R51.125 4-round split).
    core: CoreShell<V>,
    /// §5.34 preview lifecycle ledger — passed into every
    /// `pinion_rpc::dispatch` call alongside the scene. Lifecycle RPC
    /// methods read or mutate it through interior mutability;
    /// non-lifecycle methods ignore it.
    ///
    /// R51.83 §5.40 — private. Plumbed into [`DispatchContext`] by
    /// [`Self::dispatch_rpc`] only.
    previews: PreviewLedger,
    /// §5.34 R40.4 OCC revision token. `dispatch` auto-bumps on
    /// mutating RPC methods; [`ShellCore::forward`] explicitly bumps
    /// after the winit-side `invoke` since that path bypasses the
    /// dispatcher entirely.
    ///
    /// R51.83 §5.40 — private. Read-only access via
    /// [`Self::revision`] (returns the current `u64`); mutation
    /// happens through the substrate dispatch methods only.
    revision: SceneRevision,
    /// R51.53 §5.39 framework-side focus state owner. Tab/Shift+Tab
    /// traverses [`FocusManager::tab_order`] (seeded from
    /// `V::focusable_tags()` at boot); click on a tagged widget
    /// aliases [`FocusManager::focus_set`]; click on background
    /// aliases [`FocusManager::focus_clear`]. The shell consults the
    /// manager on every key dispatch so `apply_key` runs only when
    /// the widget's own tag is focused (eliminating the broadcast
    /// aliasing the pre-R51.53 design carried).
    ///
    /// R51.83 §5.40 — private. Read-only access via [`Self::focus`];
    /// mutation routed through the substrate's focus-handling
    /// methods (`handle_focus_traverse`, `click_to_focus`, …).
    focus: FocusManager,
    /// R51.53 §5.39 — abstract [`Modifiers`] cache. Refreshed by the
    /// winit-side `WindowEvent::ModifiersChanged` (converted to the
    /// `pinion_runtime::Modifiers` shape at the `app.rs` boundary);
    /// consulted on every `KeyboardInput` for Shift detection
    /// (Shift+Tab = `focus_prev`). winit emits `KeyEvent` without
    /// modifier state, so the shell has to track it out-of-band.
    ///
    /// R51.108 §5.41 — type lifted from `winit::keyboard::ModifiersState`
    /// to the substrate-local `pinion_runtime::Modifiers` so the
    /// dispatch path stays backend-agnostic for the TUI / mobile /
    /// RPC-driven input source the §2 #6 GUI/TUI dual invariant
    /// requires.
    ///
    /// R51.83 §5.40 — private. `AppShell::handle_key_press` reads
    /// only the Shift bit via [`Self::modifiers_shift_key`];
    /// mutation happens through [`Self::set_modifiers`].
    modifiers: Modifiers,
    /// R47.3 §5.36 — owned [`LayoutCache`] (LRU 256). `paint_adapter`'s
    /// Text arm consults this cache for every `Scene::Text` it walks
    /// so the view fn's static labels shape once on first paint and
    /// hit the cache on every subsequent frame. The cache also owns
    /// parley's `FontContext` / `LayoutContext` so the shell never
    /// holds parley state directly.
    ///
    /// R51.83 §5.40 — private. The vello-side paint pipeline reaches
    /// the cache through [`Self::text_cache_mut`]; substrate-internal
    /// callers (`compute_paint_scene`, `dispatch_rpc`'s producer
    /// closure) use the field directly.
    text_cache: LayoutCache,
    /// R47.7.5 §5.12 — most recent winit-rendered frame's paint scene
    /// projected into a [`LayoutNode`] tree. Refreshed at the end of
    /// every paint pass; `dispatch_rpc` hands it to
    /// `DispatchContext::with_last_paint_layout` so AI clients reach
    /// the winit-actual frame via `scene/layout {viewport: null}`.
    /// `None` until the first frame has rendered.
    ///
    /// R51.83 §5.40 — private. Set inside [`Self::finalize_frame`]
    /// and consumed inside [`Self::dispatch_rpc`].
    last_paint_layout: Option<LayoutNode>,
    /// R51.67 §5.40 — `NodeId` → widget tag map from the most recent
    /// `TreeUpdate`. Refreshed at the end of every `render` (when an
    /// adapter is attached). Consumed by `handle_action_request` so
    /// AT-side actions arriving via `AppEvent::AccessKit` resolve
    /// back to the widget tag without recomputing the tree.
    ///
    /// R51.83 §5.40 — private. Set inside [`Self::commit_access_emit`]
    /// and consumed inside [`Self::handle_action_request`].
    last_access_tag_map: HashMap<NodeId, String>,
    /// R51.72 §5.40 — previous frame's `AccessNode` set (keyed by
    /// `tag`). The next frame diffs against this to compute the
    /// dirty subset passed to `AccessTreeBuilder::dirty_tags`.
    /// AccessKit's incremental-update guidance: "an update should
    /// only include nodes that are new or changed".
    ///
    /// R51.83 §5.40 — private. Read by [`Self::plan_access_emit`],
    /// written by [`Self::commit_access_emit`].
    last_access_nodes: HashMap<String, AccessNode>,
    /// R51.72 §5.40 — `true` until the first `TreeUpdate` has been
    /// emitted (carrying the `Tree` metadata + every node). After
    /// that, subsequent emits set `initial(false)` and pass only
    /// the dirty subset.
    ///
    /// R51.83 §5.40 — private. Read by [`Self::plan_access_emit`],
    /// cleared by [`Self::commit_access_emit`].
    access_emit_initial: bool,
    /// R51.75 §5.40 — previous frame's `AccessFocus`. Compared
    /// alongside the dirty-node diff: when neither nodes nor focus
    /// changed, `update_if_active` is skipped entirely so a
    /// steady-state animation frame costs no AT-side traffic.
    ///
    /// R51.83 §5.40 — private. Read by [`Self::plan_access_emit`],
    /// written by [`Self::commit_access_emit`].
    last_access_focus: Option<AccessFocus>,
    /// R51.76 §5.40 — flag set whenever a method on
    /// [`ShellCore`] decides the next frame should repaint. Drained
    /// by [`crate::AppShell`] after each event-loop iteration and
    /// forwarded to `Window::request_redraw` when a winit `Window`
    /// is attached; remains observable for headless tests that never
    /// spin up a `Window`. The flag-based design replaces the
    /// pre-R51.76 direct `window.request_redraw()` call buried in
    /// every dispatch method, which made the substrate untestable
    /// without a real event loop.
    ///
    /// R51.83 §5.40 — private. Read-only access via
    /// [`Self::redraw_requested`]; drain via
    /// [`Self::take_redraw_request`]; mutation via
    /// [`Self::request_redraw`].
    redraw_requested: bool,

    /// R51.143 §5.28 — wall-clock timestamp of the previous
    /// [`Self::compute_paint_scene`] entry. `None` until the first
    /// paint runs; the next paint measures `now - prev` to feed both
    /// the [`Frame::with_dt`](pinion_core::Frame::with_dt) view-fn
    /// input and the
    /// [`CoreShell::tick_animations`](pinion_runtime::CoreShell::tick_animations)
    /// driver call.
    ///
    /// Per §5.28 R33 the spring solver is deterministic given
    /// `(current, velocity, target, dt, config)` — driving it from a
    /// real measured delta is what turns the synthetic substrate
    /// (which always passed `dt=0`) into a real per-frame animation
    /// pump.
    ///
    /// On the very first paint `dt = 0.0`, which leaves at-rest
    /// animations untouched and starts the spring solver from its
    /// construction baseline — the same shape any synthetic flush
    /// hits, so no special-case branching is needed elsewhere.
    last_paint_instant: Option<Instant>,
}

/// R51.77 §5.40 — pure decision returned by
/// [`ShellCore::plan_access_emit`].
///
/// Carries only the emit verdict + diff metadata: the should-emit
/// flag (`should_emit`), the initial-frame flag (`initial` — forces a
/// full tree metadata emit on the first frame), and the dirty-tag
/// set. Nodes / focus stay with the caller — the decision struct
/// borrows them while planning and lets the render path consume them
/// once for `Adapter::update_if_active` (no clone for the closure).
///
/// R51.77 split: pre-R51.77 `AccessEmitPlan` bundled the decision
/// AND the consumed nodes / focus AND mutated the `ShellCore` cache
/// inside a single `compute_access_emit` call (silent surprise —
/// pure-looking name but mutating). The textbook canonical shape
/// separates pure planning from the cache-update commit step. See
/// [`ShellCore::commit_access_emit`].
#[derive(Debug)]
pub struct AccessEmitDecision {
    /// `true` when the caller should invoke
    /// `Adapter::update_if_active`. `false` when the tree is
    /// byte-identical to the previous frame's emit (no dirty nodes,
    /// no focus change, not initial).
    pub should_emit: bool,
    /// `true` for the first emit (carries `Tree` metadata + every
    /// node). Subsequent emits set `false` and pass only the dirty
    /// subset via `AccessTreeBuilder::dirty_tags`.
    pub initial: bool,
    /// Set of tags whose `AccessNode` body (name / value / state /
    /// bounds / children) changed since the previous emit. Empty
    /// when only focus changed. On `initial` the set contains every
    /// node's tag (the AT has no prior state).
    pub dirty: HashSet<String>,
}

impl<V: WidgetView> ShellCore<V> {
    /// R51.76 §5.40 — construct the dispatch substrate with a
    /// freshly-built state scene and the initial cached state read
    /// through the §5.15 introspect channel.
    ///
    /// Identical bootstrapping to the pre-R51.76 `AppShell::new` minus
    /// the winit / wgpu / AccessKit surface (which lives on
    /// [`crate::AppShell`] and is constructed lazily on `resumed`).
    /// Headless tests build only this struct.
    ///
    /// R51.123 §5.41 — the scene + cached-state bootstrapping moves
    /// into [`CoreShell::new`] (composed via `core: CoreShell::new()`
    /// below); this constructor adds the Vello-specific extras
    /// (focus seeding, AccessKit caches, redraw flag, OCC token, RPC
    /// preview ledger, parley `LayoutCache`).
    #[must_use]
    pub fn new() -> Self {
        let core = CoreShell::<V>::new();
        // Log the initial state read through the §5.15 introspect
        // channel — same trace line shape AppShell relied on
        // pre-R51.123 so the dogfood eprintln + RPC-side observer
        // both see the boot-time state.
        eprintln!(
            "shell: initial state = {}",
            V::fmt_state_log(core.cached_state()),
        );
        // R51.53 §5.39 — seed FocusManager with the binding's
        // `focusable_tags()` enumeration. The default impl returns
        // `vec![V::tag()]` (single tab stop), which is the right
        // shape for every single-widget example; composite widgets
        // (`RadioGroup`, multi-widget views) override to enumerate
        // sub-tags or sibling widget tags.
        let mut focus = FocusManager::new();
        let tags: Vec<String> = V::focusable_tags()
            .into_iter()
            .map(str::to_owned)
            .collect();
        focus.update_focusable_tags(tags);
        Self {
            core,
            previews: PreviewLedger::default(),
            revision: SceneRevision::default(),
            focus,
            modifiers: Modifiers::empty(),
            text_cache: LayoutCache::new(),
            last_paint_layout: None,
            last_access_tag_map: HashMap::new(),
            last_access_nodes: HashMap::new(),
            access_emit_initial: true,
            last_access_focus: None,
            redraw_requested: false,
            last_paint_instant: None,
        }
    }

    /// R51.76 §5.40 — borrow the focus manager. Both tests and the
    /// vello-side paint pipeline reach the focused tag through this
    /// accessor. R51.83 §5.40: substrate-internal callers use the
    /// field directly; the surface boundary forbids it (the field
    /// itself is private).
    #[must_use]
    pub fn focus(&self) -> &FocusManager {
        &self.focus
    }
}

/// `ShellCore::new()` is the canonical constructor; the
/// `Default` impl exists so the substrate composes with any
/// future builder that defaults a member field via
/// [`Default::default`] (R51.76 — workspace lints set
/// `clippy::pedantic = "deny"`, which promotes
/// `clippy::new_without_default` to a hard build error; this
/// impl is mandatory to satisfy the lint without weakening
/// the baseline).
impl<V: WidgetView> Default for ShellCore<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: WidgetView> ShellCore<V> {

    /// R51.76 §5.40 — borrow the cached state projection. Tests
    /// observe widget state transitions through this accessor.
    /// R51.123 §5.41 — delegates to [`CoreShell::cached_state`].
    #[must_use]
    pub fn cached_state(&self) -> &V::State {
        self.core.cached_state()
    }

    /// R51.143 §5.28 — delegates to
    /// [`CoreShell::root_owner`](pinion_runtime::CoreShell::root_owner)
    /// so the view fn (and tests + SCE-emitted code) can attach
    /// [`Animation<T>`](pinion_core::Animation) instances and
    /// [`Effect`](pinion_core::Effect) closures to this binding's
    /// reactive scope.
    ///
    /// The animation list registered here is exactly the one
    /// [`Self::compute_paint_scene`] ticks once per paint cycle.
    /// Drop on `ShellCore` cascades through the wrapped
    /// [`CoreShell`](pinion_runtime::CoreShell) into the
    /// [`Owner`](pinion_core::Owner) drop semantics, cancelling every
    /// pending [`Command`](pinion_core::Command) and animation in the
    /// scope (Solid pattern, R51.137 + R51.139).
    #[must_use]
    pub fn root_owner(&self) -> &pinion_core::Owner {
        self.core.root_owner()
    }

    /// R51.76 §5.40 — current §5.34 R40.4 OCC revision counter
    /// (loaded with `Acquire` ordering — see
    /// [`SceneRevision::current`]). Mutating winit / AT-side
    /// dispatches bump it; tests assert the before/after delta when
    /// verifying that a dispatch path actually committed.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.current()
    }

    /// R51.76 §5.40 — borrow the live state scene. Tests reach the
    /// widget External through `Scene::External(node) => node.handle`
    /// when verifying introspect side effects.
    /// R51.123 §5.41 — delegates to [`CoreShell::scene`].
    #[must_use]
    pub fn scene(&self) -> &Scene {
        self.core.scene()
    }

    /// R51.76 §5.40 — drain the redraw flag set by `request_redraw`.
    ///
    /// Returns `true` once for each call to `request_redraw` between
    /// drains. [`crate::AppShell`] calls this at the end of every
    /// event-loop iteration and forwards to `Window::request_redraw`
    /// on `true`; headless tests call it directly to verify that a
    /// dispatch triggered a repaint request without standing up a
    /// `Window`.
    pub fn take_redraw_request(&mut self) -> bool {
        let r = self.redraw_requested;
        self.redraw_requested = false;
        r
    }

    /// R51.76 §5.40 — `true` when a redraw has been requested since
    /// the last drain. Tests prefer [`take_redraw_request`](Self::take_redraw_request)
    /// when they want to consume the signal; this accessor is for
    /// debug logging and peek-only assertions.
    #[must_use]
    pub fn redraw_requested(&self) -> bool {
        self.redraw_requested
    }

    /// R51.76 §5.40 — note that a repaint is required.
    ///
    /// The flag is drained by [`crate::AppShell`] once per event-loop
    /// iteration, so multiple `request_redraw` calls within one
    /// dispatch collapse to a single `Window::request_redraw` call
    /// (the textbook winit idiom: redraws are coalesced).
    pub fn request_redraw(&mut self) {
        self.redraw_requested = true;
    }

    /// R51.83 §5.40 — mutable borrow of the §5.36 [`LayoutCache`] for
    /// the vello-side paint pipeline.
    ///
    /// `paint_adapter::to_vello` walks the paint scene and consults
    /// the cache for every `Scene::Text` node (shape once, hit on
    /// every subsequent frame). The accessor is the single
    /// surface-side entry point: substrate-internal callers
    /// (`compute_paint_scene`'s `compute_layout` call) use the field
    /// directly so the surface boundary stays explicit.
    #[must_use]
    pub fn text_cache_mut(&mut self) -> &mut LayoutCache {
        &mut self.text_cache
    }

    /// R51.83 §5.40 — Shift modifier bit from the cached
    /// [`Modifiers`] state (R51.108 §5.41 winit-free).
    ///
    /// `AppShell::handle_key_press` reads the Shift bit to decide
    /// whether `Tab` calls [`Self::handle_focus_traverse`] in the
    /// reverse direction. Exposes only the bit the surface needs;
    /// the rest of the modifier state stays substrate-internal.
    #[must_use]
    pub fn modifiers_shift_key(&self) -> bool {
        self.modifiers.shift_key()
    }


    /// R51.45 §5.35 — abstract [`Touch`] dispatch (R51.108 §5.41
    /// winit-free, R51.122 §5.41 router-side lift into
    /// [`CoreShell::touch_event`]). Each finger mints a distinct
    /// [`PointerId::touch(finger_id)`] so two simultaneous touches
    /// drive two widgets without aliasing the capture lock. The
    /// substrate routes the phase-specific router calls inside
    /// [`CoreShell::touch_event`] (`Started` → `cursor_moved` +
    /// `pointer_down`; `Moved` → `cursor_moved`; `Ended` →
    /// `pointer_up` + `cursor_left`; `Cancelled` (R51.93 §5.35
    /// §5.13) → `pointer_cancel` + `cursor_left`); this method adds
    /// the Vello-only `click_to_focus` follow-up for the press phase
    /// (`Started`) so a tap on a tagged focusable widget aliases
    /// `FocusManager::focus_set`.
    fn handle_touch(&mut self, touch: Touch) -> DispatchTail<V::State> {
        let phase = touch.phase;
        let pid = PointerId::touch(touch.id);
        let tail = self.core.touch_event(touch);
        if matches!(phase, TouchPhase::Started) {
            self.click_to_focus(pid);
        }
        tail
    }

    /// Translate a typed widget event into the symbolic
    /// `invoke("send", Text(<name>))` call — the same channel the RPC
    /// `scene/invoke` route uses. Failures from the statechart
    /// (`InvokeError::Rejected` etc.) are swallowed: the SCXML decides
    /// whether a given transition fires.
    ///
    /// R51.123 §5.41 — body delegates to [`CoreShell::forward`]
    /// (which performs the `invoke` + post-dispatch tail); the
    /// Vello-side post-tail bookkeeping (OCC revision bump per §5.34
    /// R40.4, transition-log eprintln, redraw flag on state change)
    /// happens in [`Self::handle_tail`].
    pub fn forward(&mut self, event: V::Event) {
        let tail = self.core.forward(event);
        // §5.34 R40.4: winit-side input bypasses the RPC dispatcher,
        // so bump the OCC revision token directly. Spurious bumps for
        // SCXML-rejected events are acceptable per the
        // conservative-bump policy.
        self.revision.bump();
        self.handle_tail(&tail);
    }

    /// R51.37 §5.35 — route a key string through
    /// [`WidgetView::apply_key`] and, on handled
    /// (`Some(DispatchTail)` from [`CoreShell::apply_key`]), run the
    /// same post-input bookkeeping as [`Self::forward`]: bump the
    /// §5.34 revision, re-read cached state (paint on visible
    /// change), drain pending intents. Unhandled keys (`None` return)
    /// are swallowed quietly (same shape as an unmatched
    /// [`WidgetView::keybinding`]).
    pub fn apply_key(&mut self, key: &str) {
        let focused = self.focus.focused().map(str::to_owned);
        if let Some(tail) = self.core.apply_key(focused.as_deref(), key, self.modifiers) {
            self.revision.bump();
            self.handle_tail(&tail);
        }
    }

    /// R56.2.a §5.13 §5.38 — route an IME [`CompositionEvent`] through
    /// [`WidgetView::apply_composition`] and, on handled
    /// (`Some(DispatchTail)` from [`CoreShell::apply_composition`]),
    /// run the same post-input bookkeeping as [`Self::apply_key`]:
    /// bump the §5.34 revision, drain pending intents via
    /// [`Self::handle_tail`] (which re-reads cached state and
    /// requests a redraw on visible change).
    ///
    /// pinion-shell's `AppShell::window_event` `WindowEvent::Ime`
    /// arm converts winit 0.30's cross-platform
    /// [`Ime`](https://docs.rs/winit/0.30/winit/event/enum.Ime.html)
    /// enum (`Enabled` / `Preedit(text, range)` / `Commit(text)` /
    /// `Disabled`) into [`CompositionEvent`] with a `was_composing`
    /// state machine, then forwards through this method — see the
    /// `AppShell` doc comment on the arm for the mapping table.
    ///
    /// Unhandled composition events (`None` return — every widget
    /// except `TextField` and friends) are swallowed quietly; the
    /// shell does not fall through to any scroll/typeahead/other arc
    /// because composition events have no meaningful fallback (the
    /// W3C contract is "widget consumes preedit or nothing else
    /// applies").
    pub fn apply_composition(&mut self, event: &pinion_core::CompositionEvent) {
        let focused = self.focus.focused().map(str::to_owned);
        if let Some(tail) = self.core.apply_composition(focused.as_deref(), event) {
            self.revision.bump();
            self.handle_tail(&tail);
        }
    }

    /// R56.2.e §5.13 §5.22 — route a middle-mouse-button press
    /// through [`WidgetView::apply_middle_click`] and, on handled
    /// (`Some(DispatchTail)` from [`CoreShell::apply_middle_click`]),
    /// run the same post-input bookkeeping as [`Self::apply_key`]:
    /// bump the §5.34 revision, drain pending intents via
    /// [`Self::handle_tail`] (which re-reads cached state and
    /// requests a redraw on visible change).
    ///
    /// pinion-shell's `AppShell::window_event`
    /// `WindowEvent::MouseInput { button: Middle, state: Pressed, .. }`
    /// arm calls this method directly (no winit-event-to-pinion-event
    /// conversion is needed — the middle-button press has no
    /// payload beyond "happened" + the cached cursor position the
    /// `InputRouter` already holds).
    ///
    /// On the X11 / Wayland Linux desktops the canonical UX is
    /// "middle-click pastes the PRIMARY selection at the focused
    /// text widget"; [`TextField::apply_middle_click`] reads PRIMARY
    /// via the R56.2.e [`Clipboard::paste_from`] extension and
    /// inserts at the caret. On macOS / Windows the
    /// [`Clipboard::paste_from`] default impl returns `None` for
    /// `Primary` so the widget impl harmlessly produces a no-op
    /// (matching the OS-level absence of a parallel selection
    /// clipboard).
    ///
    /// Unhandled middle-clicks (`None` return — every non-text-input
    /// widget) are swallowed quietly; the shell does not fall
    /// through to any other input arc because middle-click has no
    /// meaningful fallback at the substrate level (a Vello `External`
    /// widget that wants raw `MouseButton::Middle` events can wire
    /// its own R56.2.a-style platform override, the same way IME
    /// fans out from this surface).
    pub fn middle_click(&mut self) {
        let focused = self.focus.focused().map(str::to_owned);
        if let Some(tail) = self.core.apply_middle_click(focused.as_deref(), self.modifiers) {
            self.revision.bump();
            self.handle_tail(&tail);
        }
    }

    /// R51.78 §5.39 — Tab / Shift+Tab dispatch decoupled from winit.
    ///
    /// `AppShell::handle_key_press` (winit-side) maps
    /// `Key::Named(NamedKey::Tab) + modifiers.shift_key()` into a
    /// boolean `shift` flag and forwards here. The substrate then
    /// invokes [`FocusManager::focus_next`] / [`FocusManager::focus_prev`]
    /// against the seeded `focusable_tags` order and requests a
    /// redraw when the focused tag actually changed (avoiding
    /// no-op repaints when Tab cycles back to a one-tag list).
    ///
    /// Returns the underlying `FocusManager` change flag for
    /// callers / tests that want to assert on the cycle behaviour.
    pub fn handle_focus_traverse(&mut self, shift: bool) -> bool {
        let focus_before = self.focus.focused().map(str::to_owned);
        let changed = if shift {
            self.focus.focus_prev()
        } else {
            self.focus.focus_next()
        };
        if changed {
            // R56.1.h §5.38 §5.39 — Tab / Shift+Tab traversal notifies
            // the outgoing + incoming externals of the focus change
            // before requesting the redraw so the `TextField` statechart
            // (R56.1.a) drives its Focus / Blur events in the same
            // frame that paints the new focus ring.
            self.notify_focus_change(focus_before.as_deref());
            self.request_redraw();
        }
        changed
    }

    /// R51.78 §5.37 — `Key::Character` dispatch decoupled from winit.
    ///
    /// First consults [`WidgetView::keybinding`]; on `Some(event)`
    /// routes through [`Self::forward`] (typed event channel). On
    /// `None` falls through to [`Self::apply_key`] (raw key-string
    /// dispatch). Matches the pre-R51.78 inline behaviour in
    /// `AppShell::handle_key_press` byte-for-byte.
    pub fn handle_character_key(&mut self, c: &str) {
        if let Some(ev) = V::keybinding(c) {
            self.forward(ev);
        } else {
            self.apply_key(c);
        }
    }

    /// R51.78 §5.37 — `Key::Named` dispatch decoupled from winit.
    ///
    /// `AppShell::handle_key_press` (winit-side) maps the winit
    /// `NamedKey` enum to the W3C `KeyboardEvent.key` string via
    /// [`crate::named_key_str`] and forwards the resulting
    /// `&'static str` here. The substrate routes through
    /// [`Self::apply_key`]; widgets match on the W3C string in their
    /// `apply_key` impls.
    ///
    /// `Escape` and `Tab` never reach this method — they are
    /// shell-reserved in `AppShell::handle_key_press` (`Escape` quits
    /// the window via `event_loop.exit`; `Tab` routes through
    /// [`Self::handle_focus_traverse`]).
    pub fn handle_named_key(&mut self, key_str: &str) {
        // R51.187 §5.45 R55.C.3 — give `V::apply_key` the first
        // chance on the key (widget-bound shortcut: Slider's
        // arrows, Toggle's Space, Button's Enter, etc.). If the
        // widget reports unhandled (`None` return from
        // [`CoreShell::apply_key`]), fall through to the scroll-
        // routing dispatch so an unbound arrow / page / Home / End
        // over a scroll container still scrolls. The two arcs are
        // mutually exclusive — a widget that consumes the key
        // never lets the scroll arc fire.
        let focused = self.focus.focused().map(str::to_owned);
        if let Some(tail) = self.core.apply_key(focused.as_deref(), key_str, self.modifiers) {
            self.revision.bump();
            self.handle_tail(&tail);
            return;
        }
        self.scroll_key(PointerId::MOUSE, key_str);
    }

    /// (R51.187 §5.45 R55.C.3) Keyboard scroll dispatch — the
    /// fallback path [`Self::handle_named_key`] takes when
    /// [`WidgetView::apply_key`] reports the key unhandled.
    /// Forwards through [`CoreShell::scroll_key`](pinion_runtime::CoreShell::scroll_key)
    /// which walks the deepest [`Scene::Scroll`](pinion_core::scene::Scene::Scroll)
    /// under the pointer cursor and calls
    /// [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by)
    /// / [`scroll_to`](pinion_core::widgets::scroll::ScrollState::scroll_to)
    /// depending on the key name. Requests a redraw on actual
    /// dispatch so the new offset paints next frame.
    pub fn scroll_key(&mut self, pid: PointerId, key: &str) {
        let (tail, dispatched) = self.core.scroll_key(pid, key);
        if dispatched {
            self.request_redraw();
        }
        self.handle_tail(&tail);
    }

    /// R51.80 §5.35 — winit `CursorMoved` dispatch decoupled from
    /// winit at the [`ShellCore`] surface. Forwards through
    /// [`CoreShell::cursor_moved`] (which performs the router walk +
    /// post-dispatch tail), then routes the tail through
    /// [`Self::handle_tail`].
    pub fn cursor_moved(&mut self, pid: PointerId, x: f64, y: f64) {
        let tail = self.core.cursor_moved(pid, x, y);
        self.handle_tail(&tail);
    }

    /// R51.80 §5.35 — winit `CursorLeft` dispatch decoupled from
    /// winit at the [`ShellCore`] surface.
    pub fn cursor_left(&mut self, pid: PointerId) {
        let tail = self.core.cursor_left(pid);
        self.handle_tail(&tail);
    }

    /// R51.80 §5.35 — winit `MouseInput { Pressed, Left }` dispatch.
    /// Combines [`CoreShell::pointer_down`] with the §5.39
    /// click-to-focus rule (the same path
    /// [`TouchPhase::Started`] runs after a synthetic cursor move).
    pub fn mouse_pressed(&mut self, pid: PointerId) {
        let tail = self.core.pointer_down(pid);
        self.click_to_focus(pid);
        self.handle_tail(&tail);
    }

    /// R51.80 §5.35 — winit `MouseInput { Released, Left }` dispatch.
    pub fn mouse_released(&mut self, pid: PointerId) {
        let tail = self.core.pointer_up(pid);
        self.handle_tail(&tail);
    }

    /// R51.80 §5.35 — abstract touch event dispatch (R51.108 §5.41
    /// winit-free, R51.122 §5.41 router-side lift). The surface-side
    /// `AppShell` converts a `winit::event::Touch` to [`Touch`] at
    /// the window-system boundary; future TUI / mobile / RPC paths
    /// construct the same abstract event directly. Delegates to
    /// [`Self::handle_touch`] (which calls [`CoreShell::touch_event`]
    /// plus the Vello-only `click_to_focus` follow-up on the press
    /// phase) then routes the dispatch tail.
    pub fn touch_event(&mut self, touch: Touch) {
        let tail = self.handle_touch(touch);
        self.handle_tail(&tail);
    }

    /// (R51.186 §5.45 R55.C.2) Mouse wheel dispatch — winit
    /// `WindowEvent::MouseWheel`. Forwards through
    /// [`CoreShell::wheel`](pinion_runtime::CoreShell::wheel), which
    /// walks the deepest [`Scene::Scroll`](pinion_core::scene::Scene::Scroll)
    /// under the pointer's stored cursor and calls
    /// [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by)
    /// on the attached state. winit emits `MouseWheel` without its
    /// own position field; the substrate's
    /// [`InputRouter`](pinion_runtime::InputRouter) reuses the last
    /// `CursorMoved` position for `pid` exactly the way W3C / iOS /
    /// Android specify.
    ///
    /// Requests a repaint only when the router reports an actual
    /// dispatch — silent drops (cursor outside the window, no
    /// scroll container at the point, or the covering `ScrollNode`
    /// carries no `state` link) do not bump the redraw flag, so a
    /// wheel event over a non-scrollable region cannot regress the
    /// idle-frame-skipping the R51.147 substrate guarantees.
    pub fn wheel(&mut self, pid: PointerId, delta: WheelDelta) {
        let (tail, dispatched) = self.core.wheel(pid, delta);
        if dispatched {
            self.request_redraw();
        }
        self.handle_tail(&tail);
    }

    /// R51.195 §5.49 §5.45 — drain the deferred-input inbox `dispatch`
    /// populated. Each entry replays the input through the same
    /// `cursor_moved` / `wheel` entry points the winit and TUI
    /// surfaces use, so the [`InputRouter`](pinion_runtime::InputRouter)
    /// fires under its normal post-frame redraw rules. Called once
    /// per `dispatch_rpc` after the dispatcher's `&mut scene` borrow
    /// releases.
    fn drain_deferred_inputs(&mut self, inputs: &[DeferredInput]) {
        // `DeferredInput` is `non_exhaustive`; the wildcard arm
        // covers future variants (key, cursor_only, etc.) silently
        // no-op against this drain until a follow-up round extends
        // the match.
        for input in inputs {
            match *input {
                DeferredInput::Wheel { x, y, delta } => {
                    self.cursor_moved(PointerId::MOUSE, x, y);
                    self.wheel(PointerId::MOUSE, delta);
                }
                DeferredInput::Click { x, y } => {
                    self.cursor_moved(PointerId::MOUSE, x, y);
                    self.mouse_pressed(PointerId::MOUSE);
                    self.mouse_released(PointerId::MOUSE);
                }
                DeferredInput::Key { x, y, ref key } => {
                    self.cursor_moved(PointerId::MOUSE, x, y);
                    self.handle_named_key(key);
                }
                // R660 §5.49 — `scene/drag` mirror: press at `from`,
                // march cursor linearly to `to` across `steps` frames
                // (each one forwarded to `InputRouter::cursor_moved`
                // under the R51.34 capture lock so the receiving
                // widget's `pointer_move` arc runs identically to a
                // real-mouse drag), then release. `steps == 0` lands
                // as a press / release at `from` (degenerate but
                // well-defined — RPC client gets exactly what it
                // asked for).
                DeferredInput::Drag {
                    from_x,
                    from_y,
                    to_x,
                    to_y,
                    steps,
                } => {
                    self.cursor_moved(PointerId::MOUSE, from_x, from_y);
                    self.mouse_pressed(PointerId::MOUSE);
                    if steps > 0 {
                        for step in 1..=steps {
                            let t = f64::from(step) / f64::from(steps);
                            let x = from_x + (to_x - from_x) * t;
                            let y = from_y + (to_y - from_y) * t;
                            self.cursor_moved(PointerId::MOUSE, x, y);
                        }
                    }
                    self.mouse_released(PointerId::MOUSE);
                }
                _ => {}
            }
        }
    }

    /// R51.80 §5.39 — modifier-key cache update. `KeyEvent` carries no
    /// modifier state in winit; the substrate remembers the
    /// most-recent `ModifiersChanged` so the
    /// `AppShell::handle_key_press` Tab arm can branch on Shift.
    ///
    /// R51.108 §5.41 — signature lifted from
    /// `winit::keyboard::ModifiersState` to the abstract
    /// [`Modifiers`] so the substrate stays backend-agnostic for the
    /// §2 #6 GUI/TUI dual invariant.
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// R51.80 §5.39 / R51.59 — winit `WindowEvent::Focused(true)`
    /// dispatch. ARIA Focus Order asks the framework to reinstate the
    /// previously-focused widget when the window regains focus (the
    /// [`FocusManager`] owns the snapshot). Sets `redraw_requested` when
    /// `restore` reports a change so the focus ring repaints.
    pub fn window_focused(&mut self) {
        if self.focus.restore() {
            self.request_redraw();
        }
    }

    /// R51.80 §5.39 / R51.59 — winit `WindowEvent::Focused(false)`
    /// dispatch. Saves the currently-focused widget tag so a future
    /// [`Self::window_focused`] can restore it.
    pub fn window_blurred(&mut self) {
        self.focus.save();
    }

    /// R51.80 §5.16 §5.36 — compute one frame's paint scene from the
    /// cached state.
    ///
    /// Encapsulates the per-frame pump that the surface-side render
    /// path drives every redraw:
    ///
    /// 1. Measure `dt` against the previous paint timestamp
    ///    ([`Self::last_paint_instant`]) — `0.0` on the very first
    ///    paint, otherwise `now - prev` as `f32` seconds.
    /// 2. Advance every animation attached to
    ///    [`CoreShell::root_owner`](pinion_runtime::CoreShell::root_owner)
    ///    through
    ///    [`CoreShell::tick_animations`](pinion_runtime::CoreShell::tick_animations)
    ///    so the §5.22 [`Signal`](pinion_core::Signal) values the view
    ///    fn is about to read reflect the just-elapsed slice of
    ///    real time.
    /// 3. Hand the same `dt` to [`Frame::with_dt`](pinion_core::Frame::with_dt)
    ///    so deterministic-time-dependent view-fn logic (Tween
    ///    progress reads, dt-conditional layout) sees the same delta
    ///    the spring solver did.
    /// 4. Run `V::view(state, &frame)` + the §5.21 layout pass against
    ///    the shared `text_cache`.
    ///
    /// R51.83 §5.40: the underlying `cached_state` / `text_cache`
    /// fields are private, so this method (and [`Self::text_cache_mut`]
    /// for the paint adapter borrow) is the only way for the surface
    /// to drive the pipeline. Pure with respect to substrate state
    /// modulo the timing field + `text_cache` LRU + the root owner's
    /// animation queue — every mutation is documented + tested.
    pub fn compute_paint_scene(&mut self, w: u32, h: u32) -> Scene {
        let now = Instant::now();
        let raw_dt = self
            .last_paint_instant
            .map_or(0.0_f32, |prev| now.duration_since(prev).as_secs_f32());
        self.last_paint_instant = Some(now);
        // R51.145 §5.28 — clamp before reaching the spring solver +
        // the view fn so background-resume / debugger-pause does not
        // destabilize the semi-implicit Euler integrator. Healthy
        // 60fps frames pass through unchanged; only paused / blocked
        // resumes get capped.
        let dt = clamp_frame_dt(raw_dt);
        self.core.tick_animations(dt);
        let frame = Frame::with_dt(dt);
        let cached_state = *self.core.cached_state();
        // R51.146 §5.22 — wrap the view fn in `root_owner().run(...)`
        // so [`pinion_core::Owner::current`] resolves to this
        // binding's root reactive scope from inside `V::view`.
        // Animations / Effects / Commands created without an explicit
        // [`Owner`] argument land on the framework-owned scope, dropping
        // together with this shell. The wrap is the textbook Signal /
        // Effect thread-local stack pattern; examples stay unchanged.
        let mut paint_scene = self
            .core
            .root_owner()
            .run(|| V::view(cached_state, &frame));
        let scroll_dirty =
            compute_layout_with_scroll_dirty(&mut paint_scene, &mut self.text_cache, w, h);
        // R57.X.scrollbar §5.45 — first-paint chicken-and-egg fix.
        // The layout pass writes the post-layout
        // [`ScrollState::set_max`] *after* `V::view` has already
        // produced the scene. The scrollbar widget reads `max` inside
        // `V::view` and renders thumb size as
        // `f(viewport, viewport + max)` — on the very first paint of
        // the application's lifetime `max == 0` resolves to "content
        // fits viewport" and paints a full-track thumb the user sees
        // as "scrollbar maxed out at startup". Re-running `V::view` +
        // `compute_layout` once when the layout pass actually moved
        // a bound lets the scrollbar widget pick up the freshly-
        // written max on the same paint cycle. Idempotent on
        // steady-state frames — Signal equality-skip floors
        // `scroll_dirty` at `false` and the guard short-circuits.
        if scroll_dirty {
            paint_scene = self
                .core
                .root_owner()
                .run(|| V::view(cached_state, &frame));
            compute_layout(&mut paint_scene, &mut self.text_cache, w, h);
        }
        // R51.147 §5.28 — keep painting while any animation registered
        // on the binding is still moving. `request_redraw` is
        // idempotent inside winit; the redraw flag is drained once per
        // event-loop iteration and forwarded to
        // `Window::request_redraw` when a window exists, otherwise
        // observed by headless tests. Once every animation settles
        // under the spring epsilon the call short-circuits and the
        // surface idles until the next input / state change.
        if self
            .core
            .any_animation_active(pinion_core::DEFAULT_REST_EPSILON)
        {
            self.redraw_requested = true;
        }
        paint_scene
    }

    /// R51.80 §5.40 — build the inputs to
    /// [`Self::plan_access_emit`] from a freshly-computed paint
    /// scene.
    ///
    /// Runs the pipeline `V::access_node` → `enrich_names_from_scene`
    /// → `rect_for_tag` → `V::access_focus_target` in one place so
    /// the surface-side render path does not have to interleave four
    /// reads against substrate-internal state. R51.83 §5.40: the
    /// underlying `cached_state` and `focus` fields are private, so
    /// this method is the only way for the surface to assemble the
    /// emit inputs. The pure paint scene + the substrate's read-only
    /// state (focus + `cached_state`) are the only inputs; nothing
    /// on `ShellCore` mutates.
    #[must_use]
    pub fn collect_access_emit_inputs(
        &self,
        paint_scene: &Scene,
    ) -> (Vec<AccessNode>, Option<AccessFocus>) {
        let focused = self.focus.focused().map(str::to_owned);
        let cached = self.core.cached_state();
        // (R56.1.b.1 §5.40 §5.22) `V::access_node` runs inside
        // `root_owner.run(...)` for parity with `V::view` /
        // `V::apply_key` / `V::update` (the
        // [[callback-root-owner-wrap]] family). Bindings whose a11y
        // contribution reads through reactive hooks
        // ([`use_text_edit_state`] / [`use_caret_blink`] / etc.) need
        // an active `Owner` scope to resolve the cache key; without
        // the wrap, `Owner::current()` would return `None` and the
        // hook would panic. Bindings without reactive hooks (every
        // pre-R56.1.b.1 example) are unaffected: their access_node
        // impls ignore the Owner context.
        let owner = self.core.root_owner();
        let mut nodes = owner.run(|| V::access_node(cached, focused.as_deref()));
        pinion_a11y::enrich_names_from_scene(&mut nodes, paint_scene);
        for node in &mut nodes {
            if let Some(rect) = rect_for_tag(paint_scene, &node.tag) {
                node.bounds = Some(rect);
            }
        }
        let at_focus = owner.run(|| V::access_focus_target(cached, focused.as_deref()));
        (nodes, at_focus)
    }

    /// R51.80 §5.12 §5.35 — post-render bookkeeping.
    ///
    /// Snapshots the just-rendered paint scene into the §5.12
    /// `last_paint_layout` so an AI client's
    /// `scene/layout {viewport: null}` reaches the actual frame; hands
    /// the same scene to the [`InputRouter`] so the next pointer
    /// event hit-tests against current geometry; refreshes cached
    /// state and drains pending intents (winit input bypasses the
    /// dispatcher, so the substrate has to close the loop here).
    pub fn finalize_frame(&mut self, paint_scene: Scene) {
        self.last_paint_layout = Some(build_layout_node(&paint_scene, "/0"));
        self.core.update_paint_scene(paint_scene);
        let tail = self.core.tail();
        self.handle_tail(&tail);
    }

    /// R51.53 §5.39 — click → focus auto-set / background → clear.
    /// Called after every `pointer_down` (mouse Left press or touch
    /// `TouchPhase::Started`). Mirrors the W3C HTML convention:
    /// pressing on a tagged focusable widget focuses it; pressing
    /// on background blurs the focused widget. Non-focusable tagged
    /// widgets (decoration regions that respond to hover but aren't
    /// `focusable_tags()` members) leave focus unchanged — the
    /// [`FocusManager::focus_set`] guard rejects unknown tags so
    /// the no-op falls out naturally.
    ///
    /// R56.1.h §5.38 §5.39 — focus mutation now flows through
    /// [`Self::notify_focus_change`] so any [`External`] whose
    /// focused state crossed the boundary receives
    /// [`External::on_focus_change`]. The `TextField` statechart
    /// (R56.1.a) consumes this hook to drive its Focus / Blur SCXML
    /// events and sync the [`CaretBlink`] enabled gate (R56.1.c).
    fn click_to_focus(&mut self, pid: PointerId) {
        let focus_before = self.focus.focused().map(str::to_owned);
        if let Some(target) = self.core.hover_target(pid).map(str::to_owned) {
            if !self.focus.focus_set(&target) {
                // Tagged but non-focusable (decoration) — leave focus
                // unchanged. The W3C HTML convention says only
                // focusable elements receive focus on mousedown.
            }
        } else {
            self.focus.focus_clear();
        }
        self.notify_focus_change(focus_before.as_deref());
    }

    /// R56.1.h §5.38 §5.39 — focus-change observer. Compares the
    /// pre-mutation `focus_before` snapshot to the current
    /// [`FocusManager::focused`] tag and fires
    /// [`External::on_focus_change`] on the old and new externals
    /// (if either is bound to a tagged widget in the scene tree).
    ///
    /// The two-call shape (`old.on_focus_change(false)` then
    /// `new.on_focus_change(true)`) mirrors the W3C DOM `FocusEvent`
    /// dispatch order: `blur` fires on the losing focus owner
    /// before `focus` fires on the gaining owner. Widgets that hold
    /// state across the boundary (e.g. `TextField`'s IME composition
    /// commit-on-blur path, R56.1.a) rely on this ordering so the
    /// statechart raises `text_committed` on the outgoing widget
    /// before the new widget settles into its Focused state.
    ///
    /// No-op when the focused tag did not change (the `before ==
    /// after` early return preserves the pre-R56.1.h zero-cost
    /// stance for non-focus-mutating dispatches).
    fn notify_focus_change(&mut self, focus_before: Option<&str>) {
        let focus_after_owned = self.focus.focused().map(str::to_owned);
        if focus_before == focus_after_owned.as_deref() {
            return;
        }
        let scene = self.core.scene_mut();
        if let Some(tag) = focus_before {
            if let Some(node) = scene.find_external_with_tag_mut(tag) {
                node.handle.on_focus_change(false);
            }
        }
        if let Some(tag) = focus_after_owned.as_deref() {
            if let Some(node) = scene.find_external_with_tag_mut(tag) {
                node.handle.on_focus_change(true);
            }
        }
    }

    /// Dispatch one JSON-RPC frame against the LIVE state scene.
    /// `scene/invoke /external/send PointerEnter` (and friends) drive
    /// the SCXML the same way a winit click would.
    ///
    /// R47.7.2 §5.12 — `scene/layout` requests reach the framework
    /// via `DispatchContext::with_paint_producer`: the closure captures
    /// `cached_state` (`Copy`) and `text_cache` (`&mut`), runs the
    /// view fn and `compute_layout` for the hypothetical viewport,
    /// and returns the freshly-measured paint scene.
    ///
    /// R51.76 §5.40 — `resize_request` is supplied by the caller so
    /// the substrate stays winit-free. [`crate::AppShell`] constructs
    /// the production closure (calls `Window::request_inner_size` +
    /// `Window::request_redraw`); headless tests pass a no-op.
    ///
    /// Returns the optional JSON-RPC 2.0 response frame; the caller
    /// owns the IO surface (production writes to stdout; tests
    /// inspect the string).
    ///
    /// Signature note — `&mut dyn FnMut(u32, u32)` (not generic
    /// `F: FnMut`) for two reasons: (a) the downstream
    /// [`DispatchContext::with_resize_request`] takes the same
    /// `&mut (dyn FnMut + 'a)` shape, so the substrate forwards the
    /// reference straight through without re-wrapping; (b) avoids
    /// per-callsite monomorphisation of the entire dispatch body
    /// (production callsite vs test no-op closure would otherwise
    /// duplicate ~1 KiB of code).
    pub fn dispatch_rpc(
        &mut self,
        request: &str,
        resize_request: &mut dyn FnMut(u32, u32),
    ) -> Option<String> {
        // R51.73 §5.40 — sample focus before dispatch so we can
        // detect `focus/set` (or any other focus-mutating method)
        // and trigger a redraw to refresh the focus ring.
        let focus_before = self.focus.focused().map(str::to_owned);
        let resp = {
            // Disjoint-field split mutable borrows. R51.123 §5.41 —
            // `scene` + `cached_state` live behind
            // `self.core: CoreShell<V>`; the producer closure reads
            // `cached_state` (`Copy`) and the dispatcher takes the
            // scene mut via `core.scene_mut()`. `previews` / `revision`
            // / `focus` / `text_cache` stay on the Vello extras.
            //
            // R51.146 §5.22 — clone the root [`Owner`] handle (cheap
            // `Rc` clone) before `scene_mut` so the producer closure
            // wraps `V::view` in `root_owner.run(|| ...)`. The clone
            // aliases the same reactive scope through the underlying
            // `Rc<OwnerInner>`, so animations / effects registered
            // through `Owner::current()` inside the synthetic-paint
            // path still land on the binding's owner.
            let cached_state = *self.core.cached_state();
            let root_owner = self.core.root_owner().clone();
            // R51.162 §5.23 — grab the executor Arc-clone before
            // `scene_mut` reborrows `self.core` mutably. `Arc::clone`
            // is cheap (one atomic bump) and unblocks the borrow
            // split so the dispatcher can hand `&CommandExecutor`
            // into the with_commands_executor builder below.
            let executor_for_rpc: Option<Arc<CommandExecutor>> =
                self.core.executor().cloned();
            let scene_ptr = self.core.scene_mut();
            let previews = &self.previews;
            let revision = &self.revision;
            let focus_ptr = &mut self.focus;
            let text_cache_ptr = &mut self.text_cache;
            let last_paint = self.last_paint_layout.as_ref();
            let mut produce = |w: u32, h: u32| -> Scene {
                let frame = Frame::new();
                let mut paint = root_owner.run(|| V::view(cached_state, &frame));
                // R57.X.scrollbar §5.45 — same first-paint warmup as
                // [`Self::compute_paint_scene`]. The RPC paint
                // producer feeds `scene/snapshot from: paint` and the
                // `scene/click {path}` hit-test resolution; an AI
                // client that calls `scene/snapshot` immediately
                // after launching the binary must see the same
                // first-paint-correct scrollbar that the live shell
                // surfaces. Idempotent on steady-state — Signal
                // equality-skip floors the dirty bit at false.
                if compute_layout_with_scroll_dirty(&mut paint, text_cache_ptr, w, h) {
                    paint = root_owner.run(|| V::view(cached_state, &frame));
                    compute_layout(&mut paint, text_cache_ptr, w, h);
                }
                paint
            };
            // R47.7.5 §5.12 — surface the most recent winit-rendered
            // frame to the dispatcher so `scene/layout {viewport: null}`
            // returns the actual frame snapshot. Builder pattern keeps
            // the `Option` wiring branchless at the AI-client level.
            let mut ctx = DispatchContext::new(scene_ptr, previews, revision)
                .with_paint_producer(&mut produce)
                .with_resize_request(resize_request)
                .with_focus_manager(focus_ptr);
            if let Some(snapshot) = last_paint {
                ctx = ctx.with_last_paint_layout(snapshot);
            }
            // R51.161 §5.23 — surface the root [`Owner`] handle so
            // RPC methods that read reactive substrate state can do
            // so without draining (Owner is reachable via the cloned
            // handle above; lifetime ties through the borrow split).
            // First consumer was `scene/commands`; R598 §5.50 added
            // `scene/theme_tokens` reading the cached ThemeProvider.
            ctx = ctx.with_runtime_owner(&root_owner);
            // R51.162 §5.23 — also surface the CommandExecutor for
            // in-flight introspection. The `executor_for_rpc` Arc
            // clone was taken above (before the `scene_mut` reborrow);
            // hand `&CommandExecutor` (no Arc-ness leak into
            // pinion-rpc) to the builder.
            if let Some(exec_arc) = executor_for_rpc.as_ref() {
                ctx = ctx.with_commands_executor(exec_arc.as_ref());
            }
            // R51.195 §5.49 §5.45 — wire the deferred-input inbox so
            // `scene/wheel` can enqueue events. The inbox lives on
            // the stack here; we drain it after `dispatch` returns
            // (below) so each enqueued `cursor_moved` + `wheel` lands
            // outside the dispatcher's `&mut scene` borrow.
            let mut deferred_inputs: Vec<pinion_rpc::DeferredInput> = Vec::new();
            ctx = ctx.with_deferred_inputs(&mut deferred_inputs);
            let resp = dispatch(&mut ctx, request);
            (resp, deferred_inputs)
        };
        let (resp, deferred_inputs) = resp;
        // R51.195 §5.49 §5.45 — drain the deferred-input inbox.
        // `&mut scene` is released here, so calling back into
        // `ShellCore` is legal again.
        self.drain_deferred_inputs(&deferred_inputs);
        let tail = self.core.tail();
        self.handle_tail(&tail);
        // R51.73 §5.40 — `focus/set` from the AI client must trigger
        // a redraw so the focus ring repaints on the new target. The
        // before/after comparison catches every focus-mutating
        // method without enumerating method names.
        //
        // R56.1.h §5.38 §5.39 — same boundary fires the
        // [`External::on_focus_change`] notification on the old +
        // new tags so RPC-driven `focus/set` reaches the TextField
        // statechart Focus / Blur drive on a single dispatch tick
        // (mirrors the click_to_focus and Tab paths).
        if self.focus.focused().map(str::to_owned) != focus_before {
            self.notify_focus_change(focus_before.as_deref());
            self.request_redraw();
        }
        resp
    }

    /// R51.123 §5.41 — Vello-side post-dispatch bookkeeping for a
    /// [`DispatchTail`] returned by any [`CoreShell`] dispatch
    /// method.
    ///
    /// Logs each drained §5.20 intent to stderr (the dogfood
    /// trace line shape [`crate::AppShell`] and downstream observers
    /// rely on); logs the cached-state transition (`from -> to`) and
    /// sets `redraw_requested` so the next event-loop iteration
    /// triggers a repaint when the visible state changed; both are
    /// no-ops when the tail is empty (no intents, no state change).
    /// The `scene/intents` RPC method races with this drain —
    /// whichever caller harvests first wins (poll-form,
    /// single-consumer v0 per §5.20).
    ///
    /// Takes [`DispatchTail`] by reference because the
    /// substrate-side reads (`tail.intents` iter, `tail.state_change`
    /// copy) consume nothing; the caller owns the `Vec<Intent>` and
    /// drops it on return.
    ///
    /// R51.159 §5.23 — after the existing intent + state-change
    /// bookkeeping, drain any [`pinion_core::Command`] the just-run
    /// SCXML transition queued through
    /// [`CoreShell::dispatch_pending_commands`]. With no executor
    /// installed (the default for headless tests and pre-R51.159
    /// binaries) the drain is a no-op; with an executor installed
    /// (the `run_with_handlers` entry point) every command kind with
    /// a registered handler dispatches asynchronously and the
    /// resolved [`Intent`] arrives back as
    /// [`AppEvent::IntentArrived`](crate::AppEvent::IntentArrived).
    /// Unhandled kinds are logged so a missing handler is observable.
    fn handle_tail(&mut self, tail: &DispatchTail<V::State>) {
        for intent in &tail.intents {
            eprintln!(
                "shell: intent {} payload={:?}",
                intent.tag_str(),
                intent.payload,
            );
            // R51.169 §5.23 R27 — every drained intent flows through
            // `V::update` so reducer-produced `Vec<Command>` from
            // widget-side state transitions (e.g. button.click,
            // toggle.changed) joins the same owner queue the
            // async-re-feed path uses. Closes the R27 dispatch loop's
            // input → drain → reducer arc.
            let _ = self.core.route_intent_through_update(intent);
        }
        if let Some(sc) = tail.state_change {
            eprintln!(
                "shell: state {} -> {}",
                V::fmt_state_log(&sc.before),
                V::fmt_state_log(&sc.after),
            );
            self.request_redraw();
        }
        // R51.159 §5.23 — drain Commands the dispatch arm queued.
        for cmd in self.core.dispatch_pending_commands() {
            eprintln!(
                "shell: command unhandled kind={} payload={:?}",
                cmd.kind_str(),
                cmd.payload,
            );
        }
    }

    /// R51.159 §5.23 — install or replace the
    /// [`CommandExecutor`](pinion_runtime::CommandExecutor) the
    /// substrate's [`Self::handle_tail`] drains pending
    /// [`pinion_core::Command`]s into. Forwards to
    /// [`CoreShell::set_executor`].
    pub fn set_command_executor(
        &mut self,
        executor: std::sync::Arc<CommandExecutor>,
    ) -> Option<std::sync::Arc<CommandExecutor>> {
        self.core.set_executor(executor)
    }

    /// R51.159 §5.23 — borrow the currently-installed
    /// [`CommandExecutor`]. `None` until
    /// [`Self::set_command_executor`] runs.
    #[must_use]
    pub fn command_executor(&self) -> Option<&std::sync::Arc<CommandExecutor>> {
        self.core.executor()
    }

    /// R51.159 §5.23 — re-feed a resolved [`Intent`] (arriving via
    /// [`AppEvent::IntentArrived`](crate::AppEvent::IntentArrived)
    /// from the [`ProxyIntentSink`](crate::ProxyIntentSink)) into the
    /// SCXML `send` channel.
    ///
    /// This is the closing step of the §5.23 R27 dispatch loop:
    ///
    /// ```text
    /// Owner.dispatch_command(cmd)
    ///   → CommandExecutor::dispatch → tokio worker → Intent
    ///   → ProxyIntentSink::send → AppEvent::IntentArrived
    ///   → AppShell.user_event arm → ShellCore::dispatch_intent
    ///   → Scene::External invoke("send", tag) → SCXML transition.
    /// ```
    ///
    /// R51.159 first-cut routes the intent's tag through the same
    /// `invoke("send", Text(tag))` channel typed widget events use
    /// ([`CoreShell::forward`]).
    ///
    /// R51.172 §5.23 R27 design clarification — the
    /// [`Intent::payload`](pinion_core::Intent) is NOT threaded into
    /// the SCXML invoke send by design. Pinion's split is:
    ///
    /// - **SCXML statechart** = `Model` mutation; transitions on
    ///   event *names* only (`PointerUp`, `KeyboardActivate`,
    ///   `echo.demo.echo`, …). The pinion SCXML wrapping does not
    ///   surface event data to transition guards.
    /// - **`WidgetCore::update`** (R51.166-171) = reducer; receives
    ///   the *full* [`Intent`] (tag **and** payload) and decides
    ///   what `Vec<Command>` to emit. Payload is consumed here.
    ///
    /// The R51.159-era "payload-aware SCXML send" carry resolves to
    /// "the reducer is where payload belongs". If a future widget
    /// needs payload-driven state transitions, the SCXML invoke
    /// contract would extend to `IntrospectValue::Json({"tag",
    /// "payload"})` and every `"send"` handler would learn the new
    /// shape — but no current widget has that need.
    pub fn dispatch_intent(&mut self, intent: &Intent) {
        eprintln!(
            "shell: intent-feedback {} payload={:?}",
            intent.tag_str(),
            intent.payload,
        );
        // R51.168 §5.23 R27 — reducer step: run `V::update` first so
        // any returned `Vec<Command>` lands on the root owner's queue,
        // then advance the SCXML statechart via `invoke("send", tag)`.
        // Mirrors the Elm/Iced ordering (Update before Cmd dispatch);
        // the substrate API queues the commands itself so dropping the
        // return value here is correct.
        let _ = self.core.route_intent_through_update(intent);
        if let pinion_core::Scene::External(node) = self.core.scene_mut()
            && let Some(intro) = node.handle.introspect_mut()
        {
            let _ = intro.invoke(
                "send",
                pinion_core::external::IntrospectValue::Text(intent.tag_str().to_string()),
            );
        }
        self.revision.bump();
        let tail = self.core.tail();
        self.handle_tail(&tail);
    }

    /// R51.77 §5.40 — pure planning step for the §5.40 AccessKit
    /// emit. Borrows the freshly-computed nodes + focus, consults
    /// the substrate's incremental caches, and returns the emit
    /// verdict + dirty-tag diff. **Does not mutate any
    /// `ShellCore` state** — the caller invokes
    /// [`Self::commit_access_emit`] after the `Adapter::update_if_active`
    /// closure has consumed the nodes, completing the cache update
    /// in a separate step.
    ///
    /// Two-step rationale (R51.77 split): pre-R51.77
    /// `compute_access_emit` bundled the decision AND the cache
    /// update into one `&mut self` call named like a pure function.
    /// Reading the name without reading the body suggested
    /// idempotence; two back-to-back calls actually yielded different
    /// answers (the second saw the first's cache update). The
    /// `plan_access_emit` / `commit_access_emit` pair makes the
    /// state-machine step explicit:
    ///
    /// 1. `plan_access_emit(&nodes, focus.as_ref())` — pure decision.
    /// 2. If `decision.should_emit`, feed `nodes` + `focus` into the
    ///    closure passed to `Adapter::update_if_active`.
    /// 3. `commit_access_emit(&nodes, focus.as_ref())` — advances
    ///    the cache so the next plan sees the post-emit baseline.
    ///
    /// Tests exercise the pure planner via two back-to-back
    /// `plan_access_emit` calls separated by a `commit_access_emit`
    /// without any AccessKit adapter on hand (R51.75 no-change
    /// verification path).
    #[must_use]
    pub fn plan_access_emit(
        &self,
        nodes: &[AccessNode],
        focus: Option<&AccessFocus>,
    ) -> AccessEmitDecision {
        // R51.72 §5.40 — diff against the previous frame's node
        // cache. The initial frame emits every tag (the AT has no
        // prior state); subsequent frames emit only tags whose
        // `AccessNode` body (name / value / state / bounds / children)
        // actually changed.
        let initial = self.access_emit_initial;
        let dirty: HashSet<String> = if initial {
            nodes.iter().map(|n| n.tag.clone()).collect()
        } else {
            nodes
                .iter()
                .filter(|n| self.last_access_nodes.get(&n.tag) != Some(*n))
                .map(|n| n.tag.clone())
                .collect()
        };
        // R51.75 §5.40 — no-change frame skip. Emit only when the
        // initial-frame flag is set, the dirty set is non-empty, or
        // the focus declaration shifted. Otherwise the TreeUpdate
        // would be a pure no-op (root re-emit + identical focus).
        let focus_changed = focus != self.last_access_focus.as_ref();
        let should_emit = initial || !dirty.is_empty() || focus_changed;
        AccessEmitDecision {
            should_emit,
            initial,
            dirty,
        }
    }

    /// R51.77 §5.40 — commit step paired with
    /// [`Self::plan_access_emit`]. Advances the substrate's
    /// incremental caches to the just-emitted baseline so the next
    /// planning call diffs against this frame.
    ///
    /// Always run after the `Adapter::update_if_active` closure has
    /// consumed (or borrowed) the nodes — even when
    /// `decision.should_emit` is `false`, calling `commit_access_emit`
    /// is safe (it idempotently rewrites the cache to the same
    /// values). The textbook canonical idiom is "plan, optionally
    /// emit, always commit".
    ///
    /// R51.79 §5.40 — signature takes `nodes: Vec<AccessNode>`
    /// by-value so the Vec moves straight into `last_access_nodes`
    /// without a per-node clone. Pre-R51.79 took `&[AccessNode]` and
    /// did `nodes.iter().cloned()` internally, doubling the per-frame
    /// allocation budget (one clone for the emit closure, one clone
    /// for the cache). The new shape pairs with
    /// [`AccessTreeBuilder::add`] taking `&AccessNode` — the emit
    /// closure borrows from `nodes`, then `commit_access_emit`
    /// consumes by-value: one clone per node, in the builder only.
    ///
    /// Update set: `last_access_tag_map` (`NodeId` → tag for AT-side
    /// action routing), `last_access_nodes` (per-tag snapshot for the
    /// next dirty diff — moved in by-value), `last_access_focus`
    /// (for the next focus-change detection), `access_emit_initial`
    /// (set to `false` after the first commit so the next plan emits
    /// incrementally).
    pub fn commit_access_emit(
        &mut self,
        nodes: Vec<AccessNode>,
        focus: Option<&AccessFocus>,
    ) {
        // R51.67 §5.40 — refresh the NodeId → tag map. Borrow before
        // the by-value move below.
        self.last_access_tag_map = build_tag_map(&nodes);
        // R51.79 §5.40 — move the Vec straight into the per-tag
        // HashMap. `tag.clone()` lifts only the key (a String) out;
        // each `AccessNode` itself moves without an extra clone.
        self.last_access_nodes = nodes
            .into_iter()
            .map(|n| (n.tag.clone(), n))
            .collect();
        // Refresh the focus snapshot for the next frame's
        // focus-change check.
        self.last_access_focus = focus.cloned();
        self.access_emit_initial = false;
    }

    /// R51.67 §5.40 — translate an AccessKit `ActionRequest` into a
    /// pinion-native widget intent and dispatch it through the same
    /// focus / `apply_key` substrate the winit keyboard path uses.
    /// Returns silently when the request targets the synthetic root
    /// window or an unknown `NodeId` (stale tree, AT race).
    pub fn handle_action_request(&mut self, req: &accesskit::ActionRequest) {
        let Some(action) = translate_action(req, &self.last_access_tag_map) else {
            return;
        };
        self.dispatch_access_action(&action);
    }

    /// R51.67 §5.40 — pinion-native dispatch for one AT-driven
    /// widget action.
    ///
    /// Mapping (atomic widgets):
    /// - `Focus`          → [`FocusManager::focus_set`] + redraw
    /// - `Click` / `Default` → focus + `apply_key("Enter")`
    /// - `Increment`      → focus + `apply_key("ArrowRight")`
    /// - `Decrement`      → focus + `apply_key("ArrowLeft")`
    /// - `Other`          → silent drop
    ///
    /// R51.70 §5.40 — composite child tags (containing `#`) focus
    /// the parent and route the action through
    /// [`WidgetView::access_child_invoke`] before falling back to
    /// the atomic chain. The composite parses the sub-tag (the
    /// segment after `#`) and dispatches through its own wire-format
    /// invocation path; the shell stays composite-agnostic.
    pub fn dispatch_access_action(&mut self, action: &PinionAccessAction) {
        let (parent_tag, sub_tag) = match action.tag.split_once('#') {
            Some((p, s)) => (p, Some(s)),
            None => (action.tag.as_str(), None),
        };
        match action.kind {
            AccessAction::Focus => {
                let focus_before = self.focus.focused().map(str::to_owned);
                self.focus.focus_set(parent_tag);
                self.notify_focus_change(focus_before.as_deref());
                if let Some(sub) = sub_tag {
                    // R51.82 §5.40 — composite Focus routes through
                    // the same `access_child_invoke` hook the Click
                    // path uses, so the composite can update its
                    // active-descendant model (move the visually
                    // current row inside a `RadioGroup`, light up
                    // the current item in a future `ListBox` /
                    // `MenuButton` / `TreeView`) without selecting.
                    // The composite is responsible for distinguishing
                    // Focus from Click in its impl; the shell stays
                    // composite-agnostic. Return value is observed
                    // only as "intent accepted" — Focus never falls
                    // back to the atomic `apply_key` chain (no
                    // keyboard equivalent for "make this the active
                    // descendant").
                    let _ = V::access_child_invoke(self.core.scene_mut(), sub, action.kind);
                    self.revision.bump();
                    let tail = self.core.tail();
                    self.handle_tail(&tail);
                }
                self.request_redraw();
            }
            AccessAction::Click | AccessAction::Default => {
                let focus_before = self.focus.focused().map(str::to_owned);
                self.focus.focus_set(parent_tag);
                self.notify_focus_change(focus_before.as_deref());
                if let Some(sub) = sub_tag {
                    // R51.70 §5.40 — composite child dispatch hook.
                    // The composite invokes its wire format and
                    // returns `true`; we commit the same revision /
                    // refresh / drain bookkeeping `apply_a11y_key`
                    // performs so AT-driven activation matches the
                    // keyboard path 1:1.
                    if V::access_child_invoke(self.core.scene_mut(), sub, action.kind) {
                        self.revision.bump();
                        let tail = self.core.tail();
                        self.handle_tail(&tail);
                        self.request_redraw();
                        return;
                    }
                    // Composite declined (unrecognised sub-tag /
                    // unsupported action) — fall through so the AT
                    // still sees activation feedback via the parent.
                }
                self.apply_a11y_key(parent_tag, "Enter");
            }
            AccessAction::Increment => self.apply_a11y_key(parent_tag, "ArrowRight"),
            AccessAction::Decrement => self.apply_a11y_key(parent_tag, "ArrowLeft"),
            AccessAction::Other => {}
        }
    }

    /// R51.67 §5.40 — focus + `apply_key` shared by `Click`,
    /// `Increment`, and `Decrement` arms. Mirrors the winit
    /// keyboard-path bookkeeping ([`Self::apply_key`]): bump the
    /// §5.34 OCC revision, re-read cached state, drain pending
    /// intents on handled, request a redraw regardless so the
    /// AT-side activation surfaces visually.
    ///
    /// R51.123 §5.41 — body delegates to [`CoreShell::apply_key`];
    /// the focus mutation happens before the dispatch so
    /// `core.apply_key` sees the just-focused tag in the
    /// `Some(tag)` argument.
    fn apply_a11y_key(&mut self, tag: &str, key: &str) {
        let focus_before = self.focus.focused().map(str::to_owned);
        self.focus.focus_set(tag);
        self.notify_focus_change(focus_before.as_deref());
        // R56.1.f.0 §5.13 — AT-driven activation (Click / Increment /
        // Decrement) maps to a *plain* keystroke; the AT side does not
        // surface modifier state, and no AT activation idiom (Click ↔
        // Enter, Increment ↔ ArrowRight, Decrement ↔ ArrowLeft)
        // implies any modifier. Empty Modifiers matches the W3C ARIA
        // canonical activation shape (W3C `AccessibilityFeatures` does
        // not carry a modifier slot for `Click` / `Increment` /
        // `Decrement` actions).
        let modifiers = pinion_core::Modifiers::empty();
        if let Some(tail) = self.core.apply_key(Some(tag), key, modifiers) {
            self.revision.bump();
            self.handle_tail(&tail);
        }
        self.request_redraw();
    }

}

/// R51.67 §5.40 — build the `NodeId` → widget tag map for a
/// freshly-collected list of `AccessNode`s.
///
/// Includes the synthetic root entry (`ROOT_NODE_ID` → `""`) so
/// `pinion_a11y::translate_action` can treat a root-targeted action
/// request as a sentinel and drop it without crossing into widget
/// dispatch.
///
/// R51.92 §5.40 — module-local helper (was free fn in `lib.rs`).
/// Sole caller is [`ShellCore::commit_access_emit`] above.
fn build_tag_map(nodes: &[AccessNode]) -> HashMap<NodeId, String> {
    let mut map = HashMap::with_capacity(nodes.len() + 1);
    map.insert(ROOT_NODE_ID, String::new());
    for node in nodes {
        map.insert(tag_to_node_id(&node.tag), node.tag.clone());
    }
    map
}
