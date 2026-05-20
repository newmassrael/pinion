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
//! See [[substrate-incompleteness-signal]] (the R51.29 → R51.30
//! refactor that birthed the shell) and `claim-accuracy-self-audit`
//! (the R51.80 → R51.83 → R51.92 lesson that "wrapper added" /
//! "visibility downgraded" / "module split" are three different
//! substantive depths of the same encapsulation claim).

use std::collections::{HashMap, HashSet};

use accesskit::NodeId;
use pinion_a11y::{
    tag_to_node_id, translate_action, AccessAction, AccessFocus, AccessNode,
    PinionAccessAction, ROOT_NODE_ID,
};
use pinion_core::external::IntrospectValue;
use pinion_core::{Frame, Scene, SceneRevision};
use pinion_rpc::{
    build_layout_node, dispatch, DispatchContext, LayoutNode, PreviewLedger,
};
use pinion_runtime::{
    compute_layout, rect_for_tag, walk_scene_and_drain, FocusManager, InputRouter,
    IntentQueue, Modifiers, PointerId, Touch, TouchPhase,
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
    /// Authoritative state scene — owns the live widget External via
    /// `Box<dyn External>`. Both winit input (via the `InputRouter`)
    /// and RPC dispatch (via the `DispatchContext`) reach the SCXML
    /// statechart through this single scene.
    ///
    /// R51.83 §5.40 — private. Read-only access via [`Self::scene`];
    /// mutation happens through the [`ShellCore`] dispatch methods
    /// (`forward`, `apply_key`, `cursor_moved`, …) so the substrate
    /// stays the sole writer.
    scene: Scene,
    /// Cached projection of the introspect state, kept in sync by
    /// `refresh_state` after every input. Drives change-detection
    /// for the redraw request + the view fn's input.
    ///
    /// R51.83 §5.40 — private. Read-only access via
    /// [`Self::cached_state`]; mutation happens inside `refresh_state`.
    cached_state: V::State,
    /// §5.20 intent harvest buffer. Refilled by `drain_intents` after
    /// every winit / RPC event; consumed by stderr logging. The
    /// `scene/intents` RPC method drains the same source independently
    /// since the underlying `External::pending_intents` is the single
    /// queue.
    ///
    /// R51.83 §5.40 — private. Substrate-internal harvest queue; no
    /// external observer (the RPC drain reaches the External directly).
    intent_queue: IntentQueue,
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
    /// R48 §5.35 framework-side input dispatch primitive. Owns the
    /// retained paint scene + cursor state + `hover_target` and
    /// routes pointer events to the matching `ExternalNode` in
    /// `self.scene` (the one tagged `V::tag()`).
    ///
    /// R51.83 §5.40 — private. The substrate's pointer-event wrappers
    /// (`cursor_moved`, `mouse_pressed`, …) are the only callers.
    router: InputRouter,
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
    /// callers use the field directly.
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
    #[must_use]
    pub fn new() -> Self {
        use pinion_core::scene::ExternalNode;
        // R22 §5.20: the scene-side `ExternalNode.tag` supplies the
        // widget identifier used as the intent-tag prefix. The widget
        // External itself emits the kind (e.g. "click", "toggle"); the
        // runtime walk composes `<tag>.<kind>` on drain.
        let scene = Scene::External(
            ExternalNode::new(V::create_external()).with_tag(V::tag()),
        );
        // Initial cached state via the same introspect channel
        // everything else uses — single source of truth.
        let cached_state = V::read_state(&scene);
        eprintln!(
            "shell: initial state = {}",
            V::fmt_state_log(&cached_state),
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
            scene,
            cached_state,
            intent_queue: IntentQueue::new(),
            previews: PreviewLedger::default(),
            revision: SceneRevision::default(),
            router: InputRouter::new(),
            focus,
            modifiers: Modifiers::empty(),
            text_cache: LayoutCache::new(),
            last_paint_layout: None,
            last_access_tag_map: HashMap::new(),
            last_access_nodes: HashMap::new(),
            access_emit_initial: true,
            last_access_focus: None,
            redraw_requested: false,
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
    #[must_use]
    pub fn cached_state(&self) -> &V::State {
        &self.cached_state
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
    #[must_use]
    pub fn scene(&self) -> &Scene {
        &self.scene
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
    /// winit-free). Each finger mints a distinct
    /// [`PointerId::touch(finger_id)`] so two simultaneous touches
    /// drive two widgets without aliasing the capture lock.
    ///
    /// * [`TouchPhase::Started`] runs a synthetic
    ///   [`InputRouter::cursor_moved`] first so the hover target
    ///   resolves under the press point before the
    ///   [`InputRouter::pointer_down`] lands — mirrors the mouse
    ///   case where `CursorMoved` always precedes `MouseInput`.
    /// * [`TouchPhase::Moved`] forwards the new position.
    /// * [`TouchPhase::Ended`] runs `pointer_up` then `cursor_left`
    ///   so the post-release hover refresh fires and the finger's
    ///   cursor state is dropped (a future touch with the same
    ///   finger id is a new gesture per winit's `WindowEvent::Touch`
    ///   contract).
    /// * [`TouchPhase::Cancelled`] (R51.93 §5.35 §5.13) runs
    ///   [`InputRouter::pointer_cancel`] (not `pointer_up`) so the
    ///   widget statechart sees `PointerCancel` instead of `PointerUp`
    ///   and routes `Pressed → Idle` without raising the activate
    ///   event. The trailing `cursor_left` still runs to drop the
    ///   finger's cursor state. Pre-R51.93 routed Cancelled through
    ///   `pointer_up` and silently committed `click` / `toggle` /
    ///   `selected` / `value_committed` intents the OS-revoked
    ///   gesture did not authorise (4-finger gesture, phone-call
    ///   interrupt, notification pull-down, edge-swipe back nav,
    ///   app-switcher).
    fn handle_touch(&mut self, touch: Touch) {
        let pid = PointerId::touch(touch.id);
        match touch.phase {
            TouchPhase::Started => {
                self.router.cursor_moved(pid, touch.x, touch.y, &mut self.scene);
                self.router.pointer_down(pid, &mut self.scene);
                self.click_to_focus(pid);
            }
            TouchPhase::Moved => {
                self.router.cursor_moved(pid, touch.x, touch.y, &mut self.scene);
            }
            TouchPhase::Ended => {
                self.router.pointer_up(pid, &mut self.scene);
                self.router.cursor_left(pid, &mut self.scene);
            }
            TouchPhase::Cancelled => {
                self.router.pointer_cancel(pid, &mut self.scene);
                self.router.cursor_left(pid, &mut self.scene);
            }
            // R51.108 §5.41 — `TouchPhase` is `#[non_exhaustive]`
            // for SemVer-minor variant additions (§5.13 hedge
            // precedent). Future phases (e.g. stylus hover, force
            // change) reach this arm as a no-op until an explicit
            // handler lands; ignoring is safer than panicking for
            // unknown input events.
            _ => {}
        }
    }

    /// Translate a typed widget event into the symbolic
    /// `invoke("send", Text(<name>))` call — the same channel the RPC
    /// `scene/invoke` route uses. Failures from the statechart
    /// (`InvokeError::Rejected` etc.) are swallowed: the SCXML decides
    /// whether a given transition fires.
    pub fn forward(&mut self, event: V::Event) {
        let name = V::event_name(event);
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

    /// R51.37 §5.35 — route a key string through
    /// [`WidgetView::apply_key`] and, on handled (`true`), run the
    /// same post-input bookkeeping as [`Self::forward`]: bump the
    /// §5.34 revision, re-read cached state (paint on visible
    /// change), drain pending intents. Unhandled keys are swallowed
    /// quietly (same shape as an unmatched [`WidgetView::keybinding`]).
    pub fn apply_key(&mut self, key: &str) {
        if V::apply_key(&mut self.scene, self.focus.focused(), key) {
            self.revision.bump();
            self.refresh_state();
            self.drain_intents();
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
        let changed = if shift {
            self.focus.focus_prev()
        } else {
            self.focus.focus_next()
        };
        if changed {
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
        self.apply_key(key_str);
    }

    /// R51.80 §5.35 — winit `CursorMoved` dispatch decoupled from
    /// winit at the [`ShellCore`] surface. Forwards through the
    /// [`InputRouter`] (which routes to the matching
    /// `Scene::External` via tag hit-test), then refreshes cached
    /// state + drains intents.
    pub fn cursor_moved(&mut self, pid: PointerId, x: f64, y: f64) {
        self.router.cursor_moved(pid, x, y, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.80 §5.35 — winit `CursorLeft` dispatch decoupled from
    /// winit at the [`ShellCore`] surface.
    pub fn cursor_left(&mut self, pid: PointerId) {
        self.router.cursor_left(pid, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.80 §5.35 — winit `MouseInput { Pressed, Left }` dispatch.
    /// Combines `InputRouter::pointer_down` with the §5.39
    /// click-to-focus rule (the same path
    /// `TouchPhase::Started` runs after a synthetic cursor move).
    pub fn mouse_pressed(&mut self, pid: PointerId) {
        self.router.pointer_down(pid, &mut self.scene);
        self.click_to_focus(pid);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.80 §5.35 — winit `MouseInput { Released, Left }` dispatch.
    pub fn mouse_released(&mut self, pid: PointerId) {
        self.router.pointer_up(pid, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.80 §5.35 — abstract touch event dispatch (R51.108 §5.41
    /// winit-free). The surface-side `AppShell` converts a
    /// `winit::event::Touch` to [`Touch`] at the window-system
    /// boundary; future TUI / mobile / RPC paths construct the same
    /// abstract event directly. Delegates to the multi-pointer
    /// [`Self::handle_touch`] (R51.45 §5.35) then refreshes cached
    /// state + drains intents.
    pub fn touch_event(&mut self, touch: Touch) {
        self.handle_touch(touch);
        self.refresh_state();
        self.drain_intents();
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
    /// Encapsulates `Frame::new` + `V::view(state, &frame)` +
    /// `compute_layout(&mut scene, &mut text_cache, w, h)` so the
    /// surface-side render path does not have to interleave a state
    /// read with a text-cache mutable borrow. R51.83 §5.40: the
    /// underlying `cached_state` / `text_cache` fields are private,
    /// so this method (and [`Self::text_cache_mut`] for the paint
    /// adapter borrow) is the only way for the surface to drive the
    /// pipeline. Pure with respect to substrate state (only
    /// `text_cache` mutates internally, by design — the LRU records
    /// each freshly shaped text run for the next frame's cache hit).
    pub fn compute_paint_scene(&mut self, w: u32, h: u32) -> Scene {
        let frame = Frame::new();
        let mut paint_scene = V::view(self.cached_state, &frame);
        compute_layout(&mut paint_scene, &mut self.text_cache, w, h);
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
        let mut nodes =
            V::access_node(&self.cached_state, focused.as_deref());
        pinion_a11y::enrich_names_from_scene(&mut nodes, paint_scene);
        for node in &mut nodes {
            if let Some(rect) = rect_for_tag(paint_scene, &node.tag) {
                node.bounds = Some(rect);
            }
        }
        let at_focus = V::access_focus_target(
            &self.cached_state,
            focused.as_deref(),
        );
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
        self.router.update_paint_scene(paint_scene, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
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
    fn click_to_focus(&mut self, pid: PointerId) {
        if let Some(target) = self.router.hover_target(pid).map(str::to_owned) {
            if !self.focus.focus_set(&target) {
                // Tagged but non-focusable (decoration) — leave focus
                // unchanged. The W3C HTML convention says only
                // focusable elements receive focus on mousedown.
            }
        } else {
            self.focus.focus_clear();
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
            // Disjoint-field split mutable borrows so the producer
            // closure can capture `cached_state` + `text_cache` while
            // the dispatcher still gets `scene` + `previews` + `revision`.
            let scene_ptr = &mut self.scene;
            let previews = &self.previews;
            let revision = &self.revision;
            let focus_ptr = &mut self.focus;
            let cached_state = self.cached_state;
            let text_cache_ptr = &mut self.text_cache;
            let last_paint = self.last_paint_layout.as_ref();
            let mut produce = |w: u32, h: u32| -> Scene {
                let frame = Frame::new();
                let mut paint = V::view(cached_state, &frame);
                compute_layout(&mut paint, text_cache_ptr, w, h);
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
            dispatch(&mut ctx, request)
        };
        self.refresh_state();
        self.drain_intents();
        // R51.73 §5.40 — `focus/set` from the AI client must trigger
        // a redraw so the focus ring repaints on the new target. The
        // before/after comparison catches every focus-mutating
        // method without enumerating method names.
        if self.focus.focused().map(str::to_owned) != focus_before {
            self.request_redraw();
        }
        resp
    }

    /// §5.20 live dogfood: walk the scene, drain any pending intents
    /// into the local queue, log each one to stderr. The
    /// `scene/intents` RPC method races with this drain — whichever
    /// caller harvests first wins (poll-form, single-consumer v0).
    fn drain_intents(&mut self) {
        walk_scene_and_drain(&mut self.scene, &mut self.intent_queue);
        for intent in self.intent_queue.drain() {
            eprintln!(
                "shell: intent {} payload={:?}",
                intent.tag_str(),
                intent.payload,
            );
        }
    }

    /// Re-read the cached state from the live scene; log and repaint
    /// if it changed since the previous refresh.
    fn refresh_state(&mut self) {
        let now = V::read_state(&self.scene);
        if now != self.cached_state {
            eprintln!(
                "shell: state {} -> {}",
                V::fmt_state_log(&self.cached_state),
                V::fmt_state_log(&now),
            );
            self.cached_state = now;
            self.request_redraw();
        }
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
                self.focus.focus_set(parent_tag);
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
                    let _ = V::access_child_invoke(&mut self.scene, sub, action.kind);
                    self.revision.bump();
                    self.refresh_state();
                    self.drain_intents();
                }
                self.request_redraw();
            }
            AccessAction::Click | AccessAction::Default => {
                self.focus.focus_set(parent_tag);
                if let Some(sub) = sub_tag {
                    // R51.70 §5.40 — composite child dispatch hook.
                    // The composite invokes its wire format and
                    // returns `true`; we commit the same revision /
                    // refresh / drain bookkeeping `apply_a11y_key`
                    // performs so AT-driven activation matches the
                    // keyboard path 1:1.
                    if V::access_child_invoke(&mut self.scene, sub, action.kind) {
                        self.revision.bump();
                        self.refresh_state();
                        self.drain_intents();
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
    fn apply_a11y_key(&mut self, tag: &str, key: &str) {
        self.focus.focus_set(tag);
        if V::apply_key(&mut self.scene, Some(tag), key) {
            self.revision.bump();
            self.refresh_state();
            self.drain_intents();
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
