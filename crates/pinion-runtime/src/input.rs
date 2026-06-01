//! R48 §5.35 input dispatch primitive — cursor/key → widget routing.
//!
//! [`InputRouter`] owns the framework-side input retention and dispatch
//! that R47 (hello-button hit-test fix) had to implement at the
//! application level. By moving it into pinion-runtime, every example
//! and every future widget catalog entry (R47+ Slider / Toggle /
//! `TextField`) shares the same routing — the R47-class bug (cursor on
//! background still drives the widget SCXML) cannot reappear in
//! application code because application code no longer owns the
//! routing.
//!
//! ## Lifecycle
//!
//! ```text
//!   ┌─ winit CursorMoved ──────┐
//!   │                          ▼
//!   │   router.cursor_moved(id, x, y, &mut state_scene)
//!   │       │  re-resolve hover_targets[id] from last paint scene
//!   │       │  PointerEnter/Leave dispatch on tag transition
//!   │       ▼
//!   ┌─ winit MouseInput Press ─┐
//!   │   router.pointer_down(id, &mut state_scene)
//!   │       │  PointerDown to hover_targets[id] (no-op when none)
//!   │       ▼
//!   ┌─ winit MouseInput Release┐
//!   │   router.pointer_up(id, &mut state_scene)
//!   │       │  PointerUp to hover_targets[id] (no-op when none)
//!   │       ▼
//!   ┌─ winit CursorLeft ───────┐
//!   │   router.cursor_left(id, &mut state_scene)
//!   │       │  drop cursor for id, rollback in-flight Hover
//!   │       ▼
//!   ┌─ post-render ────────────┐
//!   │   router.update_paint_scene(paint_scene, &mut state_scene)
//!   │       │  retain paint scene, refresh hover_targets for
//!   │       │  every active pointer (handles window resize moving
//!   │       │  a widget under a stationary cursor)
//!   └──────────────────────────┘
//! ```
//!
//! ## Tag matching
//!
//! The hit-test walks the *paint* scene's tagged Container / Box /
//! Path / Image / Text nodes (§5.20 [`Scene::tag`]) — these carry the
//! visual layout for the cursor to land on. The dispatch target is the
//! corresponding *state* scene's [`ExternalNode`] with the same tag —
//! that node carries the live SCXML statechart (or any other §5.15
//! introspectable handle). Application code only needs to keep the
//! two scenes' tags in sync: the same `"main_btn"` literal on the
//! paint Container and the state [`ExternalNode`].
//!
//! ## Multi-pointer (R51.38 §5.35)
//!
//! Every input method takes a [`PointerId`] identifying the source
//! pointer. Mouse-driven shells pass [`PointerId::MOUSE`]; touch /
//! pen / future input sources mint distinct ids via
//! [`PointerId::touch`]. Per-pointer state (`cursor`, `hover_target`,
//! `captured_target`) lives in `HashMap<PointerId, _>` so two
//! simultaneous touches can drag two different widgets without
//! aliasing the capture lock. Single-pointer mouse shells observe no
//! behavioural change — the maps degenerate to a single entry under
//! `PointerId::MOUSE`. This is the first-design ratify for the
//! mobile / multi-touch axis; designing in capture-aliasing-by-default
//! and refactoring later was the carry-forward path the R51.38
//! substrate-first decision rejected.
//!
//! ## Out of scope (R48+ carry-forward)
//!
//! - Multi-target dispatch (capture / bubble). The current router
//!   picks the deepest tagged ancestor and dispatches once.
//! - Focus tab order + keyboard dispatch. v0 routes pointer events
//!   only; key events stay with the application until the focus model
//!   lands (carry).
//! - Touch event wiring at the shell layer. The router's API accepts
//!   touch pointers via [`PointerId::touch`], but no `pinion-shell`
//!   call site sources them yet — winit `Touch` event integration is
//!   a separate carry.

use std::collections::HashMap;
use std::time::Instant;

use pinion_core::event::WheelDelta;
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ExternalNode, Rect, Scene};

/// R664 §5.49 — W3C UI Events `dblclick` time threshold (milliseconds).
/// Two consecutive `pointer_down` calls within this window on the same
/// target with a position delta under [`DOUBLE_CLICK_DIST_PX`] dispatch
/// a synthetic `DoubleClick` named event in addition to the second
/// `PointerDown`. 300 ms is the W3C-canonical default
/// (Web `UIEvent.detail` definition, Windows `GetDoubleClickTime`'s
/// system-tunable default, macOS `NSEvent.doubleClickInterval` default).
const DOUBLE_CLICK_TIME_MS: u128 = 300;

/// R664 §5.49 — W3C UI Events `dblclick` position tolerance (logical
/// pixels). Two consecutive presses within [`DOUBLE_CLICK_TIME_MS`] on
/// the same target must land within this Manhattan-distance window per
/// axis to qualify as a double-click; a small drag between the two
/// presses disqualifies (mirrors the Material 3 "intentional gesture"
/// + Cocoa `NSEvent.mouseLocation` tolerance). 5 logical px is the
///   `Material 3` + Cocoa convention.
const DOUBLE_CLICK_DIST_PX: f64 = 5.0;

/// R51.38 §5.35 — pointer identity used by every [`InputRouter`]
/// input method to route per-pointer cursor / hover / capture state.
/// Mouse events on every desktop platform pinion supports come from a
/// single (logical) source, so [`PointerId::MOUSE`] is a fixed `const`
/// and mouse-driven shells never allocate. Touch finger IDs route
/// through [`PointerId::touch`] which offsets by one so `PointerId(0)`
/// stays reserved for the mouse.
///
/// `Hash` + `Eq` + `Copy` so the routing tables can key on it without
/// allocation; `Debug` for diagnostic logging. The internal `u64`
/// width matches winit's `FingerId` to avoid lossy narrowing when
/// shells eventually wire touch events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerId(u64);

impl PointerId {
    /// The primary mouse pointer. Mouse events on every desktop
    /// platform are single-source, so this constant suffices —
    /// shells pass it unconditionally for every winit `CursorMoved`
    /// / `MouseInput` event. The reserved id `0` cannot collide with
    /// any [`PointerId::touch`] result because that factory offsets
    /// by one.
    pub const MOUSE: PointerId = PointerId(0);

    /// Touch-finger pointer id. The factory offsets by one so a
    /// `winit::event::Touch::id` of `0` maps to `PointerId(1)`,
    /// keeping `PointerId(0)` reserved for [`MOUSE`]. Wrapping
    /// addition handles the (theoretical) `u64::MAX` finger id edge
    /// without panic — wrap-around lands at `PointerId(0)` which
    /// then aliases the mouse, but in practice no platform mints
    /// finger ids anywhere near that magnitude.
    #[must_use]
    pub fn touch(finger_id: u64) -> Self {
        PointerId(finger_id.wrapping_add(1))
    }

    /// Raw underlying value. Exposed for diagnostic logging and for
    /// shells that mint custom synthetic pointer IDs (e.g. pen input
    /// on platforms pinion adds later). Application code that just
    /// routes mouse + touch should prefer the [`MOUSE`] constant and
    /// the [`touch`](Self::touch) factory.
    #[must_use]
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// R51.108 §5.41 — abstract pointer touch phase, mirroring winit's
/// `TouchPhase` semantics without leaking the winit dependency into
/// the substrate. The shell-side `app.rs` (winit-coupled) converts
/// `winit::event::TouchPhase` to this enum at the window-system
/// boundary; the substrate (`ShellCore`) and runtime stay
/// backend-agnostic so future TUI (§5.41) / mobile / RPC-driven input
/// paths reuse the same vocabulary without an extra translation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TouchPhase {
    /// Finger / pointer first contact (W3C `pointerdown` equivalent).
    Started,
    /// Finger / pointer position moved while contact maintained.
    Moved,
    /// Finger / pointer released cleanly (W3C `pointerup` equivalent).
    Ended,
    /// OS revoked the gesture (R51.93 §5.13 sibling of `pointer_up`) —
    /// system gesture, app switcher, notification pull-down,
    /// edge-swipe back, phone-call interrupt. Distinct from `Ended`:
    /// the substrate routes through `pointer_cancel` not `pointer_up`
    /// so the widget statechart sees `PointerCancel` and skips the
    /// click / toggle / value-committed intent the user did not
    /// authorise.
    Cancelled,
}

/// R51.108 §5.41 — pointer touch event, the abstract counterpart of
/// `winit::event::Touch`. Carries the OS-assigned finger id (raw,
/// pre-`PointerId::touch` offset), contact location in logical
/// (DPI-aware) pixels per §5.13 coord, and the phase. The shell-side
/// `app.rs` maps `winit::event::Touch::id` to this struct's `id`
/// field directly; the substrate immediately wraps via
/// `PointerId::touch(id)` so two simultaneous fingers route to
/// distinct routers without aliasing the capture lock (R51.38 §5.35).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
    /// OS-assigned finger id (raw, pre-`PointerId::touch` offset).
    pub id: u64,
    /// Contact x position in logical (DPI-aware) pixels.
    pub x: f64,
    /// Contact y position in logical (DPI-aware) pixels.
    pub y: f64,
    /// Phase of the touch lifecycle.
    pub phase: TouchPhase,
}

/// R51.108 §5.41 — abstract modifier-key state. Defined in
/// `pinion-core` since R56.1.f.0 so the widget catalog's
/// `WidgetCore::apply_key` signature can carry the four-bit modifier
/// state without inverting the crate graph; re-exported here for
/// downstream call-sites (`pinion-shell` / `pinion-tui`) that have
/// already aged on `pinion_runtime::Modifiers`. See
/// [`pinion_core::input::Modifiers`] for the canonical definition,
/// W3C `KeyboardEvent` surface mirror, and the four accessor methods
/// (`shift_key` / `control_key` / `alt_key` / `meta_key`).
pub use pinion_core::input::Modifiers;

/// Framework-side input dispatch primitive. Owns retained paint scene,
/// cursor state, and hover target; dispatches winit-side input events
/// to the state scene's matching [`ExternalNode`] via the
/// `introspect_mut().invoke("send", Text(<event name>))` channel
/// (§5.15 item 5 input forwarding).
///
/// Application code calls into the router on every winit input event
/// and once per frame to refresh the retained paint scene. The router
/// does the hit-test, decides which widget should receive each event,
/// and dispatches through the same channel that `pinion_rpc::dispatch`
/// uses for AI-driven `scene/invoke` calls — the §2 invariant #2
/// ("RPC headless as AI primary path") stays literal: a human cursor
/// and an AI agent both reach the SCXML through the same
/// `invoke("send", ...)` path.
///
/// Widget catalog R47+ (`Slider` / `Toggle` / `TextField`) plugs in by
/// attaching a tag on its paint Container and a matching tag on its
/// state [`ExternalNode`]. No application-level hit-test code is
/// needed — adding a new widget cannot reintroduce the R47-class bug
/// because the routing primitive is framework-owned.
#[derive(Debug, Default)]
pub struct InputRouter {
    /// Last-rendered paint scene (post-layout). `None` until the
    /// first [`update_paint_scene`](Self::update_paint_scene) call.
    /// The router holds it across input events so hit-tests don't
    /// need a fresh `view()` rebuild per cursor move.
    last_paint_scene: Option<Scene>,
    /// R51.38 §5.35 — per-pointer cursor position in window physical
    /// pixels. Absence means the pointer is outside the window or
    /// has never entered. Mouse-driven shells observe a single
    /// `PointerId::MOUSE` entry; touch / pen shells route each
    /// finger / stylus through its own [`PointerId`].
    cursors: HashMap<PointerId, (f64, f64)>,
    /// R51.38 §5.35 — per-pointer hover target tag. Empty when no
    /// pointer is over a tagged region. Drives `PointerEnter` /
    /// `PointerLeave` dispatch and gates `PointerDown` /
    /// `PointerUp` per pointer, so two simultaneous touches can sit
    /// on two different widgets without aliasing.
    hover_targets: HashMap<PointerId, String>,
    /// R51.34 §5.35 + R51.38 §5.35 — per-pointer capture-lock map:
    /// tag of the widget each pointer claimed on its most recent
    /// `pointer_down` via [`External::wants_pointer_capture`]. While
    /// an entry is present, every
    /// [`cursor_moved`](Self::cursor_moved) for that pointer skips
    /// [`refresh_hover`](Self::refresh_hover) and forwards the
    /// cursor position to the widget's
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move).
    /// Cleared on [`pointer_up`](Self::pointer_up); the subsequent
    /// `refresh_hover` fires the deferred `PointerLeave` if the
    /// cursor strayed off the widget during the drag. `cursor_left`
    /// is suppressed for that pointer while capture is in flight so
    /// the drag survives the window-leave / re-enter cycle. Multi-
    /// touch drags (two fingers, two widgets) each get an
    /// independent entry — the R51.38 first-design ratify avoids
    /// the aliasing-by-default refactor cost of single-target
    /// capture.
    captured_targets: HashMap<PointerId, String>,
    /// R51.40 §5.35 — per-pointer cached
    /// [`External::wants_pointer_capture`] flag for the widget under
    /// the corresponding `hover_targets` entry. Refreshed in the
    /// same hover walk as [`refresh_hover`], so [`pointer_down`]
    /// reads a bit instead of re-walking the scene tree. The cache
    /// stays consistent with the hover lifecycle: dropped when a
    /// pointer's hover clears, replaced when it moves between
    /// tagged widgets, never read while capture is in flight (the
    /// `captured_targets` map already pins the answer for that
    /// pointer). Relies on [`External::wants_pointer_capture`]
    /// being effectively constant per widget instance — the
    /// documented industry precedent (Button=false, Slider=true)
    /// and pinion's own widget catalog all return static bools.
    hover_wants_capture: HashMap<PointerId, bool>,
    /// R664 §5.49 — per-pointer "last `pointer_down` we dispatched"
    /// snapshot used by [`pointer_down`](Self::pointer_down) to detect
    /// the W3C `UIEvent.detail == 2` double-click pattern: the next
    /// press lands within [`DOUBLE_CLICK_TIME_MS`] on the same target
    /// with a position delta below [`DOUBLE_CLICK_DIST_PX`] per axis.
    /// `(instant, x, y, target_tag)` tuple — `instant` is the press
    /// timestamp, `(x, y)` is the cursor at press time (logical pixels),
    /// `target_tag` is the resolved hit-test target so a 2nd press on a
    /// *different* widget never triggers a stale double-click. Cleared
    /// after a double-click fires so the next 2-click cycle starts
    /// fresh (a triple-click is *not* the same as detail=2 + detail=3
    /// in the W3C spec — pinion sticks to the binary single/double
    /// distinction until a 2nd consumer requests triple, per
    /// `[[abstraction-needs-second-consumer]]`).
    last_press: HashMap<PointerId, (Instant, f64, f64, String)>,
}

impl InputRouter {
    /// Construct an empty router. No retained paint scene, no
    /// cursors, no hover targets, no capture locks.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current hover target tag for `id`, when any. Mainly for tests
    /// and diagnostic logging; application dispatch should not need
    /// to inspect this directly.
    #[must_use]
    pub fn hover_target(&self, id: PointerId) -> Option<&str> {
        self.hover_targets.get(&id).map(String::as_str)
    }

    /// R51.34 §5.35 — current capture-lock target tag for `id`, when
    /// that pointer claimed a widget via
    /// [`External::wants_pointer_capture`] on its most recent
    /// [`pointer_down`](Self::pointer_down). `None` when no drag is
    /// in flight for that pointer. Diagnostic / test surface only —
    /// application code never needs to inspect this directly.
    #[must_use]
    pub fn captured_target(&self, id: PointerId) -> Option<&str> {
        self.captured_targets.get(&id).map(String::as_str)
    }

    /// (R684 §5.35 §5.41 §5.16) Read-only predicate — whether the
    /// router has ever received a paint scene via
    /// [`Self::update_paint_scene`]. The substrate-level signal that
    /// the per-window `InputRouter` is "primed" for hit-testing /
    /// drag dispatch / hover resolution. Pre-R684 callers had no
    /// substrate-visible way to distinguish a never-painted router
    /// from one whose `last_paint_scene` carried an empty
    /// [`Scene::Container`]; R684 atomic 3's headless-RPC finalize
    /// uses this predicate to skip the post-dispatch finalize when
    /// the live winit paint loop already populated the router.
    #[must_use]
    pub fn has_last_paint_scene(&self) -> bool {
        self.last_paint_scene.is_some()
    }

    /// (R705 §5.12 §2 #7) Read-only borrow of the most recently
    /// painted scene — the exact tree that produced the pixels on
    /// screen (the winit paint loop stores it via
    /// [`Self::update_paint_scene`] at the end of every frame; the
    /// headless-RPC finalize stores it via [`Self::set_paint_scene`]).
    ///
    /// `scene/snapshot from: paint` serializes THIS borrow instead of
    /// re-running `V::view` at query time, so introspection equals the
    /// displayed frame *by construction* rather than by the two
    /// renderers happening to agree. Re-rendering at query time was the
    /// §2 #7 violation R705 closes: a state mutation that had not yet
    /// repainted left the screen showing one frame while a query-time
    /// re-render produced another ([[introspection-from-paint-not-screen]]).
    ///
    /// `None` until the router has received its first paint scene
    /// (never-painted window); the snapshot handler falls back to the
    /// paint producer in that bootstrap window.
    #[must_use]
    pub fn last_paint_scene(&self) -> Option<&Scene> {
        self.last_paint_scene.as_ref()
    }

    /// Update the retained paint scene after each render. Re-resolves
    /// `hover_targets` for every active pointer against the new
    /// layout — a window resize may move the button rect under a
    /// stationary cursor, and the resulting `PointerEnter` /
    /// `PointerLeave` transitions fire here so the SCXML matches the
    /// new visual state on the next frame. Pointers under capture
    /// lock keep their hover pinned (the drag invariant).
    ///
    /// (R685 §5.16 §5.35) Composition of [`Self::set_paint_scene`] (pure
    /// storage write) + [`Self::refresh_hover_for_all_active_pointers`]
    /// (synthetic hover-arc dispatch). Pre-R685 the two responsibilities
    /// were inlined here; the split lets RPC paths refresh hit-test
    /// geometry without firing synthetic hover arcs — the R660 `RadioGroup`
    /// regression bisect end-to-end (R684 atomic 3 worked around via a
    /// first-paint-only gate; R685 lands the proper substrate split so
    /// every-RPC refresh is safe).
    pub fn update_paint_scene(&mut self, scene: Scene, state_scene: &mut Scene) {
        self.set_paint_scene(scene);
        self.refresh_hover_for_all_active_pointers(state_scene);
    }

    /// (R685 §5.16 §5.35) Pure-storage paint-scene write — no
    /// `refresh_hover` side effect.
    ///
    /// Splits the R51.39 [`Self::update_paint_scene`] composition into
    /// (a) storage + (b) side-effect refresh so RPC paths that need
    /// fresh hit-test geometry **without** firing synthetic
    /// `PointerEnter` / `PointerLeave` arcs have a safe primitive.
    /// Pre-R685 the only path was the composed
    /// [`Self::update_paint_scene`], which made every per-RPC refresh
    /// double-fire hover transitions and mutate widget state in ways
    /// the application did not request (R660 `RadioGroup` End-key
    /// regression caught the issue end-to-end; R684 atomic 3 worked
    /// around it via a first-paint-only gate; R685 lands the textbook
    /// split).
    ///
    /// Use cases for the storage-only path:
    ///
    /// * RPC `scene/drag` / `scene/click` after a state change moved
    ///   the hit-test geometry — the AI client wants the next hit-test
    ///   to see fresh rects, but the synthetic hover arcs from a real
    ///   user "didn't move" the cursor (the cursor stayed put; only
    ///   the layout shifted under it).
    /// * Headless / scripted scenarios where firing input arcs would
    ///   pollute the SCXML transition log captured for assertions.
    ///
    /// The composed [`Self::update_paint_scene`] is still the right
    /// choice for live winit paint cycles (where a real
    /// `RedrawRequested` reflects either a state change with implicit
    /// pointer re-targeting, or a window resize moving widgets under a
    /// stationary cursor — both want the hover arcs).
    pub fn set_paint_scene(&mut self, scene: Scene) {
        self.last_paint_scene = Some(scene);
    }

    /// (R685 §5.16 §5.35) Refresh-hover side-effect half of the R51.39
    /// `update_paint_scene` split. Walks every non-captured pointer +
    /// re-evaluates its hover target against the current
    /// `last_paint_scene`, dispatching synthetic `PointerEnter` /
    /// `PointerLeave` if the deepest-tagged hit changed.
    ///
    /// Lives next to [`Self::set_paint_scene`] so the
    /// [`Self::update_paint_scene`] composition is mechanically
    /// derivable: write the scene, refresh hover. Pure side effect —
    /// the function takes `&mut Scene` because `refresh_hover` mutates
    /// the state scene through `dispatch_send`.
    pub fn refresh_hover_for_all_active_pointers(&mut self, state_scene: &mut Scene) {
        // Snapshot pointer ids before iterating — refresh_hover
        // takes &mut self and mutates `hover_targets`. Cloning the
        // key set keeps the multi-pointer iteration self-contained
        // (single-pointer shells: 1 entry, negligible cost).
        let ids: Vec<PointerId> = self.cursors.keys().copied().collect();
        for id in ids {
            if self.captured_targets.contains_key(&id) {
                continue;
            }
            self.refresh_hover(id, state_scene);
        }
    }

    /// winit `CursorMoved` handler. Stores the new cursor position
    /// under `id` then either:
    ///
    /// * **Capture mode** (R51.34 §5.35): when a drag-aware widget
    ///   holds the lock for this pointer, forward the cursor
    ///   position to its
    ///   [`External::pointer_move`](pinion_core::external::External::pointer_move)
    ///   as widget-relative normalised `(x_rel, y_rel)`. The hover
    ///   target stays pinned so the SCXML does not see spurious
    ///   `PointerLeave` events when the cursor strays off the
    ///   widget rect mid-drag.
    /// * **Free mode** (pre-R51.34 default): re-resolve this
    ///   pointer's hover target and dispatch `PointerEnter` /
    ///   `PointerLeave` on transitions — the canonical button-like
    ///   cancel-by-leave UX.
    pub fn cursor_moved(&mut self, id: PointerId, x: f64, y: f64, state_scene: &mut Scene) {
        self.cursors.insert(id, (x, y));
        if let Some(tag) = self.captured_targets.get(&id).cloned() {
            self.forward_pointer_move(state_scene, &tag, x, y);
        } else {
            self.refresh_hover(id, state_scene);
        }
    }

    /// winit `CursorLeft` handler. Drops the cursor for `id` and
    /// dispatches a `PointerLeave` if a hover was in flight for that
    /// pointer — *unless* a drag is in flight (R51.34 §5.35 capture
    /// lock), in which case the hover stays pinned so the drag
    /// survives the window-leave / re-enter cycle that a real
    /// drag-out gesture produces. The deferred `PointerLeave` (if
    /// the cursor never returns) fires on the matching
    /// [`pointer_up`](Self::pointer_up).
    pub fn cursor_left(&mut self, id: PointerId, state_scene: &mut Scene) {
        self.cursors.remove(&id);
        if self.captured_targets.contains_key(&id) {
            return;
        }
        if let Some(tag) = self.hover_targets.remove(&id) {
            dispatch_send(state_scene, &tag, "PointerLeave");
        }
    }

    /// winit `MouseInput` (or touch-down) press handler for `id`.
    /// Dispatches `PointerDown` to the pointer's current hover
    /// target. No-op when that pointer is over no tagged region —
    /// clicks on the background don't drive the SCXML (this is the
    /// R47 fix internalized).
    ///
    /// R51.34 §5.35: after dispatch, if the target widget opts in to
    /// pointer capture via
    /// [`External::wants_pointer_capture`](pinion_core::external::External::wants_pointer_capture),
    /// the router pins this pointer's `captured_targets` entry to
    /// that tag for the duration of the press. While pinned,
    /// [`cursor_moved`] forwards the cursor to the widget through
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move)
    /// and suppresses hover / leave dispatch for this pointer.
    /// R741 §5.35: button-like widgets now also opt in (so a click is
    /// jitter-robust) and pair it with
    /// [`External::cancel_on_release_off_target`] so a release off the
    /// widget still cancels — see [`pointer_up`](Self::pointer_up).
    ///
    /// R664 §5.49 — W3C UI Events `dblclick` detection. After the
    /// standard `PointerDown` dispatch, the router compares this
    /// press against [`last_press`](Self::last_press) for `id`: if the
    /// previous press hit the same `target_tag` within
    /// [`DOUBLE_CLICK_TIME_MS`] and the cursor moved less than
    /// [`DOUBLE_CLICK_DIST_PX`] per axis, the router synthesises a
    /// second named event `DoubleClick` to the same target on top of
    /// the normal `PointerDown`. Widgets that distinguish single from
    /// double activation handle the `DoubleClick` arm in their
    /// `invoke("send", ...)` `match`; widgets that don't (the entire
    /// pre-R664 catalogue) silently ignore the extra event so the
    /// extension is fully additive. The
    /// [`DeferredInput::DoubleClick`](pinion_rpc::dispatch::DeferredInput::DoubleClick)
    /// RPC drain reaches this same detection because its expansion
    /// fires two consecutive `pointer_down` calls with zero cursor
    /// move in between — the threshold check trivially fires for the
    /// second press, unifying the native winit and RPC-injected paths
    /// at the framework tier per [[r47-class-incident-prevention]].
    pub fn pointer_down(&mut self, id: PointerId, state_scene: &mut Scene) {
        if let Some(tag) = self.hover_targets.get(&id).cloned() {
            dispatch_send(state_scene, &tag, "PointerDown");
            // R51.40 §5.35 — read the cached wants_capture bit
            // populated by the matching `refresh_hover` instead of
            // re-walking the state-scene tree. The cache is
            // populated when the pointer enters this tag and
            // cleared on leave, so it is always consistent with the
            // current `hover_targets[id]`.
            let wants = self
                .hover_wants_capture
                .get(&id)
                .copied()
                .unwrap_or(false);
            if wants {
                self.captured_targets.insert(id, tag.clone());
                // R51.35 §5.35 — click-to-position: forward the
                // press-time cursor as the initial `pointer_move` so
                // a click-without-drag still seeds the widget's
                // value at the click point (Material / `SwiftUI` / Qt
                // Slider click-jumps-to-position UX). Without this
                // forward the value would not update unless the user
                // also dragged the cursor at least one pixel.
                if let Some(&(x, y)) = self.cursors.get(&id) {
                    self.forward_pointer_move(state_scene, &tag, x, y);
                }
            }

            // R664 §5.49 — double-click detection. Same target +
            // within W3C `dblclick` time + space window → synthesise
            // a `DoubleClick` named event on top of `PointerDown`.
            let now = Instant::now();
            let cursor = self.cursors.get(&id).copied();
            let is_double = match (self.last_press.get(&id), cursor) {
                (Some(prev), Some((cx, cy))) => {
                    let elapsed = now.duration_since(prev.0).as_millis();
                    let dx = (prev.1 - cx).abs();
                    let dy = (prev.2 - cy).abs();
                    prev.3 == tag
                        && elapsed < DOUBLE_CLICK_TIME_MS
                        && dx < DOUBLE_CLICK_DIST_PX
                        && dy < DOUBLE_CLICK_DIST_PX
                }
                _ => false,
            };
            if is_double {
                dispatch_send(state_scene, &tag, "DoubleClick");
                // Detail=2 fired; the next press starts a fresh cycle
                // (no rolling triple-click — pinion stops at binary
                // single/double until a 2nd consumer surfaces).
                self.last_press.remove(&id);
            } else if let Some((cx, cy)) = cursor {
                self.last_press.insert(id, (now, cx, cy, tag));
            }
        }
    }

    /// winit `MouseInput` (or touch-up) release handler for `id`.
    /// Dispatches `PointerUp` to that pointer's current hover
    /// target. Release with the cursor off-button is a no-op in
    /// free mode: `cursor_moved`'s `PointerLeave` already drove the
    /// SCXML out of `Pressed` back to `Idle`.
    ///
    /// R51.34 §5.35: in capture mode the cursor may currently sit
    /// off the widget rect (the drag strayed, or a button-like press
    /// slid off). The release event then depends on the captured
    /// widget's [`External::cancel_on_release_off_target`] policy:
    ///
    /// * `false` (drag widgets, e.g. Slider) — always dispatch
    ///   `PointerUp` so the drag commits its value wherever the cursor
    ///   ended (`Dragging → Hover` → `value_committed`).
    /// * `true` (R741 button-like widgets) — dispatch `PointerUp`
    ///   (activate) only when the cursor is still over the captured
    ///   tag, else dispatch `PointerLeave` (cancel). This is the
    ///   "slide off to abort" gesture; capture made it reachable by
    ///   suppressing the mid-press stray leave.
    ///
    /// Capture for this pointer is then released and
    /// [`refresh_hover`](Self::refresh_hover) re-runs to resettle the
    /// hover state against the release position.
    pub fn pointer_up(&mut self, id: PointerId, state_scene: &mut Scene) {
        if let Some(cap_tag) = self.captured_targets.get(&id).cloned() {
            let release_over = self.cursor_over_tag(id, &cap_tag);
            let event = if !release_over && widget_cancels_on_release_off(state_scene, &cap_tag) {
                "PointerLeave"
            } else {
                "PointerUp"
            };
            dispatch_send(state_scene, &cap_tag, event);
            self.captured_targets.remove(&id);
            self.refresh_hover(id, state_scene);
        } else if let Some(tag) = self.hover_targets.get(&id).cloned() {
            // Free (no-capture) release: the cursor is over the target
            // (a mid-press stray already drove the SCXML out of Pressed
            // via `cursor_moved`'s `PointerLeave`).
            dispatch_send(state_scene, &tag, "PointerUp");
        }
    }

    /// R741 §5.35 — whether the cursor for `id` currently resolves to
    /// `tag` (full-tag equality, so a composite `group#0` press that is
    /// released over `group#1` reads as off-target — the W3C "press and
    /// release on the same control" rule). `false` when the pointer has
    /// no tracked cursor or no last paint scene.
    fn cursor_over_tag(&self, id: PointerId, tag: &str) -> bool {
        match (self.cursors.get(&id), self.last_paint_scene.as_ref()) {
            (Some(&(x, y)), Some(scene)) => resolve_hover_tag(scene, x, y).as_deref() == Some(tag),
            _ => false,
        }
    }

    /// (R51.186 §5.45 R55.C.2) Mouse wheel input dispatch.
    ///
    /// Forwards the wheel delta to the deepest
    /// [`ScrollNode`](pinion_core::scene::ScrollNode) covering the
    /// pointer's last-known cursor position whose
    /// `state: Option<Rc<ScrollState>>` link is wired. `Pixels`
    /// route through verbatim; `Lines` multiply by [`LINE_HEIGHT_PX`]
    /// (16, the W3C / browser default) before `f32 → i32`
    /// round-to-nearest. The translated `(dx, dy)` pair feeds
    /// [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by),
    /// which clamps against the declared bounds and fires the
    /// reactive `Signal::set` — the next paint cycle re-runs the
    /// view fn against the updated offset without any
    /// application-level wiring.
    ///
    /// W3C sign convention: positive `dy` scrolls *downward*
    /// (content shifts up visually); positive `dx` scrolls
    /// *rightward*. The convention matches
    /// [`WheelDelta`](pinion_core::event::WheelDelta) and the
    /// `ScrollState` offset semantics directly — no per-axis
    /// inversion at this boundary.
    ///
    /// No-op (returns `false`) when any of the following hold:
    ///
    /// - The pointer `id` has no stored cursor (cursor never
    ///   entered the window for this pointer, or `cursor_left`
    ///   already dropped it). winit / web / iOS all emit wheel
    ///   events without their own position field — they reuse
    ///   the surface's tracked cursor, so the router does the
    ///   same.
    /// - The retained paint scene is unset (wheel fired before
    ///   the first frame).
    /// - No `Scene::Scroll` covers the cursor point.
    /// - The covering `ScrollNode` has no `state` attached (a
    ///   declarative-only scroll node the application built
    ///   without `with_state(...)` — the router silently drops
    ///   the wheel rather than panicking).
    ///
    /// Returns `true` when the wheel was dispatched against an
    /// attached `ScrollState`. Backends (Vello: `ShellCore::wheel`;
    /// TUI: `ShellCoreTui::wheel`) use the return to decide
    /// whether to request a repaint — silent drops never bump the
    /// redraw flag.
    pub fn wheel(&mut self, id: PointerId, delta: WheelDelta) -> bool {
        let Some(&(x, y)) = self.cursors.get(&id) else {
            return false;
        };
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return false;
        };
        let xu = floor_clamp_u32(x);
        let yu = floor_clamp_u32(y);
        let Some(state) = paint.scroll_state_at(xu, yu) else {
            return false;
        };
        let (dx, dy) = wheel_delta_to_pixels(delta);
        state.scroll_by(dx, dy);
        true
    }

    /// (R51.187 §5.45 R55.C.3) Keyboard scroll input dispatch.
    ///
    /// Routes a W3C `KeyboardEvent.key` string into the deepest
    /// [`ScrollNode`](pinion_core::scene::ScrollNode) covering the
    /// pointer's last cursor position whose `state` link is wired.
    /// Eight key names are recognised; any other string is a no-op
    /// (returns `false`) so the caller's regular `apply_key`
    /// dispatch arm can stay the primary path for widget-bound keys
    /// and the scroll router only acts on unhandled scrolling
    /// shortcuts.
    ///
    /// | Key           | Effect                                       |
    /// |---------------|----------------------------------------------|
    /// | `ArrowDown`   | `scroll_by(0, +LINE_HEIGHT_PX)`              |
    /// | `ArrowUp`     | `scroll_by(0, -LINE_HEIGHT_PX)`              |
    /// | `ArrowRight`  | `scroll_by(+LINE_HEIGHT_PX, 0)`              |
    /// | `ArrowLeft`   | `scroll_by(-LINE_HEIGHT_PX, 0)`              |
    /// | `PageDown`    | `scroll_by(0, +viewport.h)` (1-page step)    |
    /// | `PageUp`      | `scroll_by(0, -viewport.h)`                  |
    /// | `Home`        | `scroll_to(offset_x, 0)` (y-axis to top)     |
    /// | `End`         | `scroll_to(offset_x, max_y)` (y-axis bottom) |
    ///
    /// The arrow / page deltas honour the W3C `WheelEvent` sign
    /// convention: positive `dy` scrolls downward (content shifts
    /// up visually). `Home` / `End` preserve the horizontal offset
    /// — they match the W3C "vertical extreme" semantics every
    /// desktop scroll container uses; a future round adds
    /// `Ctrl+Home` / `Ctrl+End` for the (0, 0) / (max, max)
    /// corner cases (carry).
    ///
    /// No-op (returns `false`) when any of the same router-state
    /// conditions [`Self::wheel`] checks hold: no stored cursor for
    /// `id`, no retained paint scene, no `Scene::Scroll` covers the
    /// cursor, or the covering node has no `state` link.
    /// Application-level key routing reads the `false` and lets
    /// the regular [`pinion_core::WidgetCore::apply_key`] dispatch
    /// stay the primary path. Returns `true` when the key was
    /// recognised AND a scroll dispatched.
    pub fn scroll_key(&mut self, id: PointerId, key: &str) -> bool {
        let Some(&(x, y)) = self.cursors.get(&id) else {
            return false;
        };
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return false;
        };
        let xu = floor_clamp_u32(x);
        let yu = floor_clamp_u32(y);
        let Some(scroll_node) = paint.scroll_target_at(xu, yu) else {
            return false;
        };
        let Some(state) = scroll_node.state.as_ref() else {
            return false;
        };
        let line: i32 = LINE_HEIGHT_PX_I32;
        // `viewport.h` / `viewport.w` are `u32` from `Rect`; clamp
        // into `i32` for the page step. Real-world viewports never
        // exceed `i32::MAX` (which would imply a 2-billion-pixel
        // window); the `try_from` fallback keeps the math defined
        // for the adversarial extreme.
        let page_y: i32 = i32::try_from(scroll_node.viewport.h).unwrap_or(i32::MAX);
        let page_x: i32 = i32::try_from(scroll_node.viewport.w).unwrap_or(i32::MAX);
        match key {
            "ArrowDown" => state.scroll_by(0, line),
            "ArrowUp" => state.scroll_by(0, -line),
            "ArrowRight" => state.scroll_by(line, 0),
            "ArrowLeft" => state.scroll_by(-line, 0),
            "PageDown" => state.scroll_by(0, page_y),
            "PageUp" => state.scroll_by(0, -page_y),
            "Home" => {
                let (ox, _) = state.offset();
                state.scroll_to(ox, 0);
            }
            "End" => {
                let (ox, _) = state.offset();
                let (_, my) = state.max();
                state.scroll_to(ox, my);
            }
            // Horizontal Home/End / Ctrl-modifier extensions are
            // future R55.C.4 carry. Silence other keys so the
            // caller's regular `apply_key` arm stays primary.
            _ => {
                // Suppress page_x to avoid unused-binding warnings
                // until a horizontal Page sub-axis lands.
                let _ = page_x;
                return false;
            }
        }
        true
    }

    /// R51.93 §5.35 — pointer cancellation handler. The OS-side
    /// counterpart to [`pointer_up`](Self::pointer_up): the user did
    /// **not** release the pointer of their own accord, the system
    /// revoked the gesture. winit emits `TouchPhase::Cancelled` for
    /// every such revoke path (4-finger system gesture, phone-call
    /// interrupt, notification banner pull-down, app-switcher
    /// invocation, edge-swipe back nav, Android `MotionEvent.ACTION_CANCEL`,
    /// iOS `UITouch` cancellation, etc.).
    ///
    /// Dispatches `PointerCancel` to the pointer's current hover or
    /// captured target. Widget statecharts route `Pressed → Idle` on
    /// this event **without raising the activate event**, so the
    /// `click` / `toggle` / `selected` / `value_committed` intent
    /// never fires for a cancelled gesture. Capture release + hover
    /// refresh mirror [`Self::pointer_up`]'s post-dispatch
    /// bookkeeping so the substrate's trailing `cursor_left` lands
    /// cleanly.
    ///
    /// Free-mode pre-R51.93 (touch cancel routed via `pointer_up`)
    /// silently committed a click the user did not authorise — this
    /// method is the textbook fix.
    pub fn pointer_cancel(&mut self, id: PointerId, state_scene: &mut Scene) {
        let target = self
            .hover_targets
            .get(&id)
            .cloned()
            .or_else(|| self.captured_targets.get(&id).cloned());
        if let Some(tag) = target {
            dispatch_send(state_scene, &tag, "PointerCancel");
        }
        if self.captured_targets.remove(&id).is_some() {
            self.refresh_hover(id, state_scene);
        }
    }

    /// R51.34 §5.35 — capture-mode cursor forward. Look up the
    /// post-layout rect of the captured widget in the retained paint
    /// scene, normalise the cursor `(x, y)` into widget-relative
    /// `[0.0, 1.0]` coordinates (may exceed when the cursor strays),
    /// and hand them to the captured `External` via
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move).
    /// Silent no-op when the paint scene is unset (cursor moved
    /// before the first frame) or the tag is unmappable to a rect.
    ///
    /// R51.42 §5.35 — composite hit-target paint tags (`"group#i"`)
    /// resolve the rect on the raw paint tag (the sub-region under
    /// the pointer) and the state-scene `External` on the primary
    /// half (the single composite handle). Capture-aware composite
    /// widgets are out of scope for the R51.41 RFC — `RadioGroup`
    /// returns `wants_pointer_capture = false` — but the wiring is
    /// kept symmetric so any future drag-aware composite slots in
    /// without revisiting the input router.
    fn forward_pointer_move(
        &self,
        state_scene: &mut Scene,
        target_tag: &str,
        cursor_x: f64,
        cursor_y: f64,
    ) {
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return;
        };
        let (primary, _) = split_subindex(target_tag);
        let Some(external) = find_external_by_tag(state_scene, primary) else {
            return;
        };
        // R738 §5.35 — choose the normalization rect per the widget's
        // opt-in. Default: the captured (sub-)tag's own rect — correct
        // for single-tag capture widgets (primary == target_tag) and for
        // composites whose drag value is sub-region-relative (dock
        // tear-off). A widget whose value spans the whole widget (the
        // range slider) returns `true` and normalizes against the primary
        // (track) rect, so grabbing a thumb sub-tag still maps the cursor
        // across the full track instead of saturating on the thumb rect.
        let norm_tag = if external.handle.capture_normalize_against_primary() {
            primary
        } else {
            target_tag
        };
        let Some(rect) = rect_for_tag(paint, norm_tag) else {
            return;
        };
        let (x_rel, y_rel) = normalize_cursor(rect, cursor_x, cursor_y);
        external.handle.pointer_move(x_rel, y_rel);
    }

    /// Recompute `hover_targets[id]` from `id`'s current cursor and
    /// the retained paint scene. Dispatches `PointerLeave` for the
    /// pointer's old target (if any) then `PointerEnter` for its new
    /// target (if any) so consumers always see the leave-before-
    /// enter ordering even when the cursor crosses directly from one
    /// tagged widget to another. Per-pointer ordering — two
    /// pointers crossing different widgets see two independent
    /// enter / leave streams.
    ///
    /// R51.40 §5.35: the new target's
    /// [`External::wants_pointer_capture`] is queried in the same
    /// pass and cached in `hover_wants_capture` so the next
    /// [`pointer_down`] reads a bit instead of re-walking the
    /// state-scene tree.
    fn refresh_hover(&mut self, id: PointerId, state_scene: &mut Scene) {
        let now = match (self.cursors.get(&id), &self.last_paint_scene) {
            (Some(&(x, y)), Some(scene)) => resolve_hover_tag(scene, x, y),
            _ => None,
        };
        let prev = self.hover_targets.get(&id).cloned();
        if prev == now {
            return;
        }
        if let Some(prev_tag) = prev {
            self.hover_targets.remove(&id);
            self.hover_wants_capture.remove(&id);
            dispatch_send(state_scene, &prev_tag, "PointerLeave");
        }
        if let Some(target) = now {
            self.hover_targets.insert(id, target.clone());
            let wants = widget_wants_capture(state_scene, &target);
            self.hover_wants_capture.insert(id, wants);
            dispatch_send(state_scene, &target, "PointerEnter");
        }
    }
}

/// Hit-test `paint_scene` at `(x, y)` and return the deepest tagged
/// ancestor's tag. Returns `None` when no node in the hit path
/// carries a tag (the cursor is over a fully untagged region —
/// usually the background, possibly some untagged decoration).
///
/// The walk is deepest-first because the visual nesting matches the
/// expected dispatch target — a tagged label inside a tagged button
/// dispatches to the label first (if anyone tags labels), falling
/// back to the button container.
fn resolve_hover_tag(paint_scene: &Scene, x: f64, y: f64) -> Option<String> {
    let xu = floor_clamp_u32(x);
    let yu = floor_clamp_u32(y);
    let hit = paint_scene.hit_test(xu, yu)?;
    // Walk segments deepest-first: the longer the prefix, the deeper
    // the ancestor. The root (empty prefix) is the last fallback.
    for k in (0..=hit.segments.len()).rev() {
        let Some(scene) = paint_scene.lookup_path_ref(&hit.segments[..k]) else {
            continue;
        };
        if let Some(tag) = scene.tag() {
            return Some(tag.to_string());
        }
    }
    None
}

/// Dispatch a synthetic input event to the state scene's matching
/// `ExternalNode`. Walks the state scene depth-first; calls
/// `introspect_mut().invoke("send", Text(event_name))` on the first
/// node whose `tag` equals `target_tag`. Silent no-op when no
/// matching node is found — application's view-scene tag and
/// state-scene tag are out of sync, but routing keeps running rather
/// than panic.
///
/// R51.42 §5.35 — when `target_tag` carries a `'#'` sub-index suffix
/// (paint `"group#2"` for the composite hit-target convention), the
/// state-scene lookup uses the primary half (`"group"`) and the wire
/// payload is rewritten to the `"<idx>:<EventName>"` format
/// (`"2:PointerEnter"`) that composite widgets like `RadioGroup`
/// parse in their own `invoke("send", ...)` handler.
fn dispatch_send(state_scene: &mut Scene, target_tag: &str, event_name: &str) {
    let (primary, sub_index) = split_subindex(target_tag);
    let Some(external) = find_external_by_tag(state_scene, primary) else {
        return;
    };
    let Some(intro) = external.handle.introspect_mut() else {
        return;
    };
    let payload = match sub_index {
        Some(idx) => format!("{idx}:{event_name}"),
        None => event_name.to_string(),
    };
    let _ = intro.invoke("send", IntrospectValue::Text(payload));
}

/// R51.42 §5.35 — split a paint tag into `(primary, sub_index)`
/// according to the composite hit-target convention. Paint
/// `"group#2"` → `("group", Some("2"))` — state-scene `ExternalNode`
/// lookup uses `"group"`; the sub-index prefixes the wire payload to
/// `invoke("send", ...)` per the R51.41 RFC. Plain tag `"main_btn"`
/// → `("main_btn", None)` — backwards-compatible single-tag flow.
/// Degenerate `"tag#"` (trailing `#` with empty sub-index) collapses
/// to `("tag", None)` so the dispatch path does not forward a
/// malformed `":<EventName>"` payload that composite widgets would
/// reject; the application's paint-tag schema is treated as opaque
/// and the router never panics on a degenerate input.
fn split_subindex(tag: &str) -> (&str, Option<&str>) {
    match tag.split_once('#') {
        Some((primary, idx)) if !idx.is_empty() => (primary, Some(idx)),
        Some((primary, _)) => (primary, None),
        None => (tag, None),
    }
}

/// Depth-first search for an [`ExternalNode`] whose tag matches
/// `target_tag`. Returns the first match in declaration order
/// (matches [`walk_scene_and_drain`](crate::walk_scene_and_drain)'s
/// traversal direction). Containers recurse; non-container variants
/// compare their own tag (when applicable) and stop.
fn find_external_by_tag<'a>(scene: &'a mut Scene, target_tag: &str) -> Option<&'a mut ExternalNode> {
    match scene {
        Scene::External(node) => {
            if tag_matches(node.tag.as_deref(), target_tag) {
                Some(node)
            } else {
                None
            }
        }
        Scene::Container(c) => {
            for child in &mut c.children {
                if let Some(found) = find_external_by_tag(child, target_tag) {
                    return Some(found);
                }
            }
            None
        }
        // Box / Text / Path / Image / Effect cannot carry an
        // `External` handle, so they never produce a dispatch target.
        _ => None,
    }
}

/// Tag comparison helper. `ExternalNode.tag` is `Option<Cow<...>>`;
/// resolve the borrow then string-compare.
fn tag_matches(node_tag: Option<&str>, target: &str) -> bool {
    matches!(node_tag, Some(t) if t == target)
}

/// R51.34 §5.35 — the **window-absolute** post-layout rect of the tagged
/// primitive named by `target_tag`. `None` when no node carries the tag
/// or it is scrolled fully out of view.
///
/// R51.62 §5.40 — `pub` so `pinion-shell` can resolve post-layout widget
/// bounds when lowering [`pinion_a11y::AccessNode`] into
/// `accesskit::TreeUpdate`; also used by the router's pointer-capture
/// move ([`InputRouter::dispatch_pointer_move_to`]).
///
/// R705.1 §5.45 §2 #7 — delegates to the single coordinate-translation
/// authority [`Scene::rect_for_tag_absolute`]. Pre-R705.1 this was a
/// scroll-BLIND walk (recursed `Container` but not `Scroll`), so a
/// widget inside a [`Scene::Scroll`] (a listbox row, a tree row)
/// returned `None` — silently denying it AccessKit bounds (AT could not
/// locate it) and breaking pointer-capture normalization. The delegate
/// now translates by the enclosing scroll offsets + clips to the
/// viewport stack, exactly like the focus-ring overlay and the RPC
/// click resolver — one walker, no scroll-blind divergence.
#[must_use]
pub fn rect_for_tag(scene: &Scene, target_tag: &str) -> Option<Rect> {
    scene.rect_for_tag_absolute(target_tag)
}

/// R51.34 §5.35 — normalise a winit cursor `(f64, f64)` into
/// widget-relative `(f32, f32)` over `rect`. `0.0` maps to the
/// left / top edge, `1.0` to the right / bottom edge. Coordinates
/// may exceed `[0.0, 1.0]` (or be negative) when the cursor strays
/// outside the rect under R51.34 capture lock — Slider clamps in
/// its [`pointer_move`](pinion_core::external::External::pointer_move)
/// impl, future drag widgets may not. Zero-size rect (degenerate
/// layout) collapses to `(0.0, 0.0)` so consumers never divide by
/// zero.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn normalize_cursor(rect: Rect, cursor_x: f64, cursor_y: f64) -> (f32, f32) {
    let width = f64::from(rect.w);
    let height = f64::from(rect.h);
    let x_rel = if width > 0.0 {
        ((cursor_x - f64::from(rect.x)) / width) as f32
    } else {
        0.0
    };
    let y_rel = if height > 0.0 {
        ((cursor_y - f64::from(rect.y)) / height) as f32
    } else {
        0.0
    };
    (x_rel, y_rel)
}

/// R51.34 §5.35 — ask the state-scene `ExternalNode` matching
/// `target_tag` whether it opts in to pointer capture via
/// [`External::wants_pointer_capture`](pinion_core::external::External::wants_pointer_capture).
/// `false` when no matching node is found (out-of-sync paint and
/// state tags) so the router never claims capture on a phantom
/// widget.
///
/// R51.42 §5.35 — composite hit-target paint tags (`"group#i"`)
/// route the state-scene lookup through the primary half so the
/// single composite `External` decides capture once for the whole
/// hit-region. The sub-index is discarded here because capture is
/// a property of the composite handle, not of any one sub-region.
fn widget_wants_capture(state_scene: &Scene, target_tag: &str) -> bool {
    let (primary, _) = split_subindex(target_tag);
    widget_wants_capture_walk(state_scene, primary).unwrap_or(false)
}

/// R741 §5.35 — resolve [`External::cancel_on_release_off_target`] for
/// the external registered at `target_tag`'s primary half. `false` when
/// the tag is not found or the widget keeps the drag-commit default.
fn widget_cancels_on_release_off(state_scene: &Scene, target_tag: &str) -> bool {
    let (primary, _) = split_subindex(target_tag);
    widget_cancels_on_release_off_walk(state_scene, primary).unwrap_or(false)
}

/// Recursive helper for [`widget_cancels_on_release_off`] (mirror of
/// [`widget_wants_capture_walk`]).
fn widget_cancels_on_release_off_walk(scene: &Scene, target_tag: &str) -> Option<bool> {
    match scene {
        Scene::External(node) => {
            if tag_matches(node.tag.as_deref(), target_tag) {
                Some(node.handle.cancel_on_release_off_target())
            } else {
                None
            }
        }
        Scene::Container(c) => {
            for child in &c.children {
                if let Some(found) = widget_cancels_on_release_off_walk(child, target_tag) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Recursive helper for [`widget_wants_capture`]. Returns
/// `Some(bool)` when the tag is found (allowing the caller to
/// distinguish "found, but declined" from "not found"), `None`
/// when the walk finds no match.
fn widget_wants_capture_walk(scene: &Scene, target_tag: &str) -> Option<bool> {
    match scene {
        Scene::External(node) => {
            if tag_matches(node.tag.as_deref(), target_tag) {
                Some(node.handle.wants_pointer_capture())
            } else {
                None
            }
        }
        Scene::Container(c) => {
            for child in &c.children {
                if let Some(found) = widget_wants_capture_walk(child, target_tag) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Saturating cast from a winit cursor coordinate (`f64`) to the
/// `u32` accepted by [`Scene::hit_test`]. Negative values clamp to
/// `0` (cursor can never hit at sub-zero coords); fractional
/// precision is dropped (hit-test resolution is whole pixels at
/// R48). The allow-list documents what the saturating clamp protects
/// against, keeping the lint silenced only at this one call site.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn floor_clamp_u32(v: f64) -> u32 {
    v.max(0.0) as u32
}

/// (R51.186 §5.45 R55.C.2) Default line-height in logical pixels
/// used to convert
/// [`WheelDelta::Lines`](pinion_core::event::WheelDelta::Lines)
/// into the integer scroll offset
/// [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by)
/// expects. The 16-pixel value matches the W3C `WheelEvent`
/// default (`window.devicePixelRatio == 1.0`) and Chromium /
/// Firefox / Safari `wheel` event handling on every desktop
/// platform — callers and tests reading wheel-driven offset
/// deltas can rely on the value as the framework constant. A
/// per-widget override (custom line-height for monospace text
/// containers, etc.) is a carry-forward sub-axis (R55.C.4) that
/// lands on top of this constant without breaking the existing
/// API surface.
pub const LINE_HEIGHT_PX: f32 = 16.0;

/// (R51.187 §5.45 R55.C.3) Integer mirror of [`LINE_HEIGHT_PX`]
/// for the arrow-key step in
/// [`InputRouter::scroll_key`](crate::input::InputRouter::scroll_key).
/// Hard-coded so the cast happens at compile time rather than
/// the (unsafe-at-saturation) `f32 as i32` path on every arrow
/// keypress.
const LINE_HEIGHT_PX_I32: i32 = 16;

/// (R51.186 §5.45 R55.C.2) Convert a unit-tagged
/// [`WheelDelta`](pinion_core::event::WheelDelta) into the
/// `(dx, dy)` integer pixel pair `ScrollState::scroll_by`
/// expects. `Pixels` route through verbatim; `Lines` multiply
/// by [`LINE_HEIGHT_PX`]. Both axes round to the nearest pixel
/// and saturate at `i32` boundaries; `NaN` clamps to zero so an
/// adversarial input never produces a wrap.
fn wheel_delta_to_pixels(delta: WheelDelta) -> (i32, i32) {
    let (fx, fy) = match delta {
        WheelDelta::Pixels { dx, dy } => (dx, dy),
        WheelDelta::Lines { dx, dy } => (dx * LINE_HEIGHT_PX, dy * LINE_HEIGHT_PX),
        // R55.C.2 — `WheelDelta` is `#[non_exhaustive]`; an
        // unknown future variant (e.g. `Pages` for PgUp / PgDn
        // coarse scroll) degrades to a zero delta rather than
        // panicking. The substrate must stay robust against
        // a `pinion-core` bump that introduces a variant the
        // running `pinion-runtime` does not yet recognise. The
        // R55.C.* sub-axis cascade adds the explicit arms as
        // each variant gains a defined offset semantics.
        _ => (0.0, 0.0),
    };
    (round_clamp_i32(fx), round_clamp_i32(fy))
}

/// (R51.186 §5.45 R55.C.2) Round-to-nearest `f32 → i32` with
/// `NaN`-guard. Rust's `f32 as i32` saturates at the integer
/// boundaries since 1.45 and converts `NaN` to `0`; the explicit
/// `is_nan` check documents the policy at the call site rather
/// than relying on the silent language-level fallback (matches
/// the R51.145 `clamp_frame_dt` `NaN`-guard precedent).
#[allow(clippy::cast_possible_truncation)]
fn round_clamp_i32(v: f32) -> i32 {
    if v.is_nan() {
        return 0;
    }
    v.round() as i32
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use pinion_core::external::{
        Backend, BackendFallback, BackendSupport, ExternalIntrospect, InterveneError,
        IntrospectSchema, InvokeError, RepaintOwner, ThreadOwnership,
    };
    use pinion_core::scene::{ContainerNode, Rect};
    use pinion_core::style::{BoxStyle, Color};

    /// Shared-state stub External — every `invoke("send", Text(name))`
    /// pushes `name` onto the held `Vec`. Constructed with
    /// [`CaptureExternal::new`] which returns the External *and* a
    /// matching `Arc<Mutex<...>>` handle the test holds for
    /// assertion. The router moves the External into an
    /// `ExternalNode`; the test keeps the Arc clone to read what
    /// arrived without re-extracting from the scene tree.
    struct CaptureExternal {
        captures: Arc<Mutex<Vec<String>>>,
    }

    impl CaptureExternal {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let captures = Arc::new(Mutex::new(Vec::new()));
            (Self { captures: Arc::clone(&captures) }, captures)
        }
    }

    impl std::fmt::Debug for CaptureExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("CaptureExternal").finish()
        }
    }

    impl pinion_core::external::External for CaptureExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for CaptureExternal {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[])
        }
        fn query(&self, _path: &str) -> Option<IntrospectValue> {
            None
        }
        fn intervene(
            &mut self,
            _path: &str,
            _value: IntrospectValue,
        ) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
        fn invoke(
            &mut self,
            method: &str,
            args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            if method == "send" {
                if let IntrospectValue::Text(name) = args {
                    self.captures.lock().expect("mutex poisoned").push(name);
                }
            }
            Ok(IntrospectValue::Null)
        }
    }

    fn read(captures: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        captures.lock().expect("mutex poisoned").clone()
    }

    /// Build a paint scene with one tagged button container of fixed
    /// size, centered in a wider background container. Matches the
    /// hello-button shape so tests use realistic coordinates.
    fn paint_with_button(viewport_w: u32, viewport_h: u32, btn_rect: Rect) -> Scene {
        let button = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag("main_btn")
                .with_style(BoxStyle::filled(Color::default())),
        );
        // Manually set button rect (skip taffy layout for unit-test
        // determinism; this is the post-layout artifact the router
        // would normally receive from `compute_layout`).
        let mut button_with_rect = button;
        if let Scene::Container(c) = &mut button_with_rect {
            c.rect = btn_rect;
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![button_with_rect])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    /// Build a state scene with one [`ExternalNode`] tagged `main_btn`
    /// (`CaptureExternal` inside) — the dispatch target for the paint
    /// scene above. Returns the `Arc<Mutex>` handle so tests inspect
    /// the captures without re-walking the scene tree.
    fn state_with_button() -> (Scene, Arc<Mutex<Vec<String>>>) {
        let (capture, captures) = CaptureExternal::new();
        let scene = Scene::External(
            ExternalNode::new(Box::new(capture)).with_tag("main_btn"),
        );
        (scene, captures)
    }

    #[test]
    fn cursor_off_button_does_not_dispatch() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Cursor at (10, 10) — far from the button rect (80..120 x 80..120).
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(read(&captures).is_empty());
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn r741_captured_button_release_over_activates_release_off_cancels() {
        use pinion_core::external::IntrospectValue;
        use pinion_core::widgets::toggle::ToggleExternal;

        fn toggle_value(scene: &Scene) -> bool {
            let Scene::External(node) = scene else { panic!("external root") };
            matches!(
                node.handle.introspect().unwrap().query("value"),
                Some(IntrospectValue::Bool(true))
            )
        }
        fn fresh() -> (InputRouter, Scene) {
            let mut router = InputRouter::new();
            // The toggle reuses the `main_btn` tag so `paint_with_button`
            // supplies its post-layout rect (80..120 x 80..120).
            let mut state = Scene::External(
                ExternalNode::new(Box::new(ToggleExternal::new())).with_tag("main_btn"),
            );
            router.update_paint_scene(
                paint_with_button(200, 200, Rect::new(80, 80, 40, 40)),
                &mut state,
            );
            (router, state)
        }

        // ACTIVATE — press + release both over the captured toggle flips it.
        let (mut router, mut state) = fresh();
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("main_btn"),
            "button captures the pointer on press (R741)");
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(toggle_value(&state), "release over the captured button activates");

        // JITTER — a stray *back onto* the widget before release still
        // activates (capture suppressed the mid-press PointerLeave).
        let (mut router, mut state) = fresh();
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 121.0, 100.0, &mut state); // 1px off
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // back on
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(toggle_value(&state), "sub-pixel jitter during press does not cancel");

        // CANCEL — a deliberate slide off the widget then release cancels.
        let (mut router, mut state) = fresh();
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // slid off
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(!toggle_value(&state), "release off the captured button cancels");
    }

    #[test]
    fn cursor_on_button_dispatches_enter_then_down_up() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Cursor on the button rect center.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerDown".into(), "PointerUp".into()],
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
    }

    #[test]
    fn cursor_crossing_off_button_fires_leave() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // on
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // off
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn cursor_left_rolls_back_hover() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // on
        router.cursor_left(PointerId::MOUSE, &mut state); // window-leave
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn pointer_down_off_button_is_noop() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // No cursor_moved — cursor stays None.
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn pointer_down_before_first_paint_is_noop() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        // CursorMoved arrives before update_paint_scene — common at
        // startup. last_paint_scene is None, so hover_target stays
        // None, so dispatch is suppressed.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn resize_shifts_button_under_stationary_cursor() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        // First frame: button at (80..120) — cursor at (100, 100) hits.
        router.update_paint_scene(
            paint_with_button(200, 200, Rect::new(80, 80, 40, 40)),
            &mut state,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        // Window resize moves the button to (10..50). Cursor stays at
        // (100, 100) — now off the button. update_paint_scene must
        // re-resolve and emit PointerLeave.
        router.update_paint_scene(
            paint_with_button(200, 200, Rect::new(10, 10, 40, 40)),
            &mut state,
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
    }

    #[test]
    fn dispatch_to_missing_state_tag_is_silent() {
        let mut router = InputRouter::new();
        // State has a different tag than the paint scene's button.
        let (capture, captures) = CaptureExternal::new();
        let mut state = Scene::External(
            ExternalNode::new(Box::new(capture)).with_tag("other_widget"),
        );
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        // hover_target resolves to "main_btn" from paint, but state
        // has no matching ExternalNode → silent no-op.
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn floor_clamp_u32_handles_negative_and_fractional() {
        assert_eq!(floor_clamp_u32(-1.0), 0);
        assert_eq!(floor_clamp_u32(0.0), 0);
        assert_eq!(floor_clamp_u32(1.9), 1);
        assert_eq!(floor_clamp_u32(99.5), 99);
    }

    // ─── R51.34 §5.35 capture-lock fixtures + tests ────────────

    /// Shared event log alias — symbolic input event names captured
    /// via `invoke("send", Text(<name>))` calls (the full symbolic
    /// path the router uses for `PointerEnter` / `PointerDown` /
    /// `PointerUp` / `PointerLeave`). Tests hold an `Arc` clone for
    /// assertions; the router moves the External into an
    /// `ExternalNode`.
    type EventLog = Arc<Mutex<Vec<String>>>;

    /// Shared move log alias — `(x_rel, y_rel)` tuples captured via
    /// `External::pointer_move` during capture lock. Only the
    /// `DragCaptureExternal` fixture appends here.
    type MoveLog = Arc<Mutex<Vec<(f32, f32)>>>;

    /// Drag-aware capture fixture. Opts in to pointer capture via
    /// [`External::wants_pointer_capture`] and records every
    /// [`External::pointer_move`] forward (so tests can assert the
    /// router fed the correct widget-relative normalised coords).
    /// Symbolic events (`PointerEnter` / `Down` / `Up` / `Leave`)
    /// share the same `events` log as the existing
    /// [`CaptureExternal`] for cross-correlation assertions in
    /// drag-end sequences.
    struct DragCaptureExternal {
        events: EventLog,
        moves: MoveLog,
        // R738 — when true, `capture_normalize_against_primary` returns
        // true (range-slider-style whole-widget normalization).
        normalize_primary: bool,
    }

    impl DragCaptureExternal {
        fn new() -> (Self, EventLog, MoveLog) {
            Self::with_normalize_primary(false)
        }

        fn with_normalize_primary(normalize_primary: bool) -> (Self, EventLog, MoveLog) {
            let events = Arc::new(Mutex::new(Vec::new()));
            let moves = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    events: Arc::clone(&events),
                    moves: Arc::clone(&moves),
                    normalize_primary,
                },
                events,
                moves,
            )
        }
    }

    impl std::fmt::Debug for DragCaptureExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("DragCaptureExternal").finish()
        }
    }

    impl pinion_core::external::External for DragCaptureExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn wants_pointer_capture(&self) -> bool {
            true
        }
        fn capture_normalize_against_primary(&self) -> bool {
            self.normalize_primary
        }
        fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
            self.moves.lock().expect("mutex poisoned").push((x_rel, y_rel));
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for DragCaptureExternal {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[])
        }
        fn query(&self, _path: &str) -> Option<IntrospectValue> {
            None
        }
        fn intervene(
            &mut self,
            _path: &str,
            _value: IntrospectValue,
        ) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
        fn invoke(
            &mut self,
            method: &str,
            args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            if method == "send" {
                if let IntrospectValue::Text(name) = args {
                    self.events.lock().expect("mutex poisoned").push(name);
                }
            }
            Ok(IntrospectValue::Null)
        }
    }

    fn read_moves(moves: &MoveLog) -> Vec<(f32, f32)> {
        moves.lock().expect("mutex poisoned").clone()
    }

    /// Paint scene mirroring [`paint_with_button`] but with the
    /// `main_slider` tag — the drag-widget counterpart of the
    /// button-like fixture.
    fn paint_with_slider(viewport_w: u32, viewport_h: u32, slider_rect: Rect) -> Scene {
        let slider = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag("main_slider")
                .with_style(BoxStyle::filled(Color::default())),
        );
        let mut slider_with_rect = slider;
        if let Scene::Container(c) = &mut slider_with_rect {
            c.rect = slider_rect;
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![slider_with_rect])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    fn state_with_slider() -> (Scene, EventLog, MoveLog) {
        let (capture, events, moves) = DragCaptureExternal::new();
        let scene = Scene::External(
            ExternalNode::new(Box::new(capture)).with_tag("main_slider"),
        );
        (scene, events, moves)
    }

    #[test]
    fn capture_lock_pins_hover_during_drag() {
        // Drag-aware widget: cursor stray off rect during press must
        // NOT fire PointerLeave. The SCXML must stay in its `Dragging`
        // state through the strays, ending only on pointer_up.
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // PointerEnter
        router.pointer_down(PointerId::MOUSE, &mut state); // PointerDown + capture lock
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("main_slider"));
        router.cursor_moved(PointerId::MOUSE, 200.0, 200.0, &mut state); // stray off
        // No PointerLeave during stray — capture lock keeps the
        // hover pinned. Only PointerEnter + PointerDown so far.
        assert_eq!(
            read(&events),
            vec!["PointerEnter".to_string(), "PointerDown".into()],
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // back over
        // Still no extra events (the router is in capture mode,
        // hover doesn't re-resolve).
        assert_eq!(
            read(&events),
            vec!["PointerEnter".to_string(), "PointerDown".into()],
        );
        router.pointer_up(PointerId::MOUSE, &mut state);
        // PointerUp lands now; capture clears; subsequent refresh
        // sees cursor (100, 100) IS on the rect — no PointerLeave.
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
    }

    #[test]
    fn capture_lock_forwards_pointer_move_normalized() {
        // During capture, cursor_moved must forward the cursor as
        // widget-relative normalised coords. Rect (80, 80, 40, 40)
        // means cursor (100, 100) → ((100 - 80) / 40, (100 - 80) / 40)
        // = (0.5, 0.5). The R51.35 click-to-position patch makes
        // pointer_down forward the press-time cursor too, so the
        // press at (100, 100) emits a (0.5, 0.5) entry before the
        // three drag-time moves below.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // PointerEnter (not capture-mode yet)
        assert!(read_moves(&moves).is_empty());
        router.pointer_down(PointerId::MOUSE, &mut state); // enter capture + click-point forward (0.5, 0.5)
        router.cursor_moved(PointerId::MOUSE, 80.0, 80.0, &mut state); // top-left
        router.cursor_moved(PointerId::MOUSE, 120.0, 120.0, &mut state); // bottom-right
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // centre
        let log = read_moves(&moves);
        assert_eq!(log.len(), 4);
        assert!((log[0].0 - 0.5).abs() < 1e-4 && (log[0].1 - 0.5).abs() < 1e-4);
        assert!((log[1].0 - 0.0).abs() < 1e-4 && (log[1].1 - 0.0).abs() < 1e-4);
        assert!((log[2].0 - 1.0).abs() < 1e-4 && (log[2].1 - 1.0).abs() < 1e-4);
        assert!((log[3].0 - 0.5).abs() < 1e-4 && (log[3].1 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn pointer_down_forwards_initial_cursor() {
        // R51.35 §5.35 — click-without-drag still updates the
        // widget's value. The Slider UX precedent: clicking on the
        // track jumps the thumb to the click point even if the user
        // releases without moving the mouse.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Click at x = 110 → x_rel = (110 - 80) / 40 = 0.75.
        router.cursor_moved(PointerId::MOUSE, 110.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read_moves(&moves);
        // Exactly one pointer_move (the click-point); no drag moves
        // because the cursor never moved between down and up.
        assert_eq!(log.len(), 1);
        assert!((log[0].0 - 0.75).abs() < 1e-4);
        assert!((log[0].1 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn capture_lock_allows_coords_outside_rect() {
        // Stray off the widget under capture lock — coords may exceed
        // [0, 1] or be negative; the consumer (Slider) clamps in its
        // own pointer_move impl. R51.35 click-to-position prepends a
        // (0.5, 0.5) press-time entry; the two strays follow.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state); // click-point (0.5, 0.5)
        router.cursor_moved(PointerId::MOUSE, 40.0, 100.0, &mut state); // x = -1.0
        router.cursor_moved(PointerId::MOUSE, 160.0, 100.0, &mut state); // x = 2.0
        let log = read_moves(&moves);
        assert_eq!(log.len(), 3);
        assert!((log[0].0 - 0.5).abs() < 1e-4);
        assert!((log[1].0 - (-1.0)).abs() < 1e-4);
        assert!((log[2].0 - 2.0).abs() < 1e-4);
    }

    #[test]
    fn cursor_left_during_capture_keeps_drag_alive() {
        // Cursor leaves the window while a drag is in flight (the
        // user dragged the mouse off-screen). The router must
        // suppress PointerLeave; the drag resumes when the cursor
        // re-enters. The eventual pointer_up still dispatches.
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_left(PointerId::MOUSE, &mut state); // off-screen
        // No PointerLeave; capture still pinned.
        assert_eq!(
            read(&events),
            vec!["PointerEnter".to_string(), "PointerDown".into()],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("main_slider"));
        // Drag resumes when cursor re-enters.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
    }

    #[test]
    fn pointer_up_off_widget_dispatches_then_fires_leave() {
        // Drag ended outside the widget rect. PointerUp dispatches
        // to the captured tag (Slider observes Dragging → Hover →
        // value_committed); then the post-release refresh_hover
        // dispatches the deferred PointerLeave (Hover → Idle).
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // stray off
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
                "PointerLeave".into(),
            ],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn button_like_widget_preserves_pre_r51_34_cancel_by_leave() {
        // Regression: a non-capturing widget (default
        // wants_pointer_capture = false) must still cancel by leave
        // — cursor stray off during press fires PointerLeave, and
        // pointer_up off-button is a no-op (existing R47 behaviour).
        let mut router = InputRouter::new();
        let (mut state, events) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // PointerLeave
        router.pointer_up(PointerId::MOUSE, &mut state); // no dispatch (hover gone)
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerLeave".into(),
            ],
        );
    }

    #[test]
    fn capture_pointer_up_with_no_hover_or_capture_is_silent() {
        // pointer_up called with nothing pressed and no capture →
        // no dispatch (existing R47 behaviour). Defensive — winit
        // can replay key events on focus regain.
        let mut router = InputRouter::new();
        let (mut state, events) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(read(&events).is_empty());
    }

    #[test]
    fn normalize_cursor_handles_zero_size_rect() {
        // Degenerate layout (e.g. a Slider that hasn't laid out yet)
        // collapses to (0, 0); the router must not divide by zero.
        let rect = Rect::new(10, 10, 0, 0);
        let (x_rel, y_rel) = normalize_cursor(rect, 5.0, 5.0);
        assert!((x_rel - 0.0).abs() < f32::EPSILON);
        assert!((y_rel - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rect_for_tag_returns_inner_when_matched() {
        // rect_for_tag finds the tagged child's rect, not the root.
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        let found = rect_for_tag(&paint, "main_btn").expect("tag present");
        assert_eq!(found, Rect::new(80, 80, 40, 40));
        // Missing tag → None.
        assert!(rect_for_tag(&paint, "ghost").is_none());
    }

    #[test]
    fn capture_lock_skips_when_paint_scene_unset() {
        // Capture entered before the first paint (winit replay edge
        // case). cursor_moved finds no rect → pointer_move silent.
        // The router does not panic.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        // pointer_down without paint → hover_target is None →
        // no PointerDown → no capture. Verify that direct invariant
        // first.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
        // Now seed the paint scene + simulate a successful press to
        // enter capture; then drop the paint scene state by NOT
        // calling update_paint_scene with a fresh rect — the router
        // still holds last_paint_scene from the previous call.
        // (We can't easily clear last_paint_scene from outside; this
        // test instead validates pointer_down before paint does NOT
        // claim capture, exercising the same defensive path.)
        assert!(read_moves(&moves).is_empty());
    }

    // ─── R51.38 §5.35 multi-pointer fixtures + tests ───────────

    /// Paint scene with two drag-aware widgets — `slider_a` on the
    /// left and `slider_b` on the right. Used by the multi-touch
    /// drag tests to exercise the per-pointer capture map.
    fn paint_with_two_sliders(viewport_w: u32, viewport_h: u32) -> Scene {
        let slider_a = {
            let mut s = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("slider_a")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut s {
                c.rect = Rect::new(20, 20, 60, 60);
            }
            s
        };
        let slider_b = {
            let mut s = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("slider_b")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut s {
                c.rect = Rect::new(120, 20, 60, 60);
            }
            s
        };
        let mut root = Scene::Container(
            ContainerNode::new(vec![slider_a, slider_b])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    /// State scene with two drag-aware externals matching the paint
    /// fixture above. Returns both event + move logs so tests can
    /// distinguish which widget received what.
    #[allow(clippy::type_complexity)]
    fn state_with_two_sliders() -> (Scene, (EventLog, MoveLog), (EventLog, MoveLog)) {
        let (a, ea, ma) = DragCaptureExternal::new();
        let (b, eb, mb) = DragCaptureExternal::new();
        let root = Scene::Container(
            ContainerNode::new(vec![
                Scene::External(ExternalNode::new(Box::new(a)).with_tag("slider_a")),
                Scene::External(ExternalNode::new(Box::new(b)).with_tag("slider_b")),
            ])
            .with_style(BoxStyle::filled(Color::default())),
        );
        (root, (ea, ma), (eb, mb))
    }

    #[test]
    fn pointer_id_mouse_is_reserved_zero() {
        // Backwards-compat invariant: mouse pointer maps to the
        // reserved `PointerId(0)` slot; touch finger ids offset by
        // one so they never alias the mouse no matter what winit
        // hands the router.
        assert_eq!(PointerId::MOUSE.raw(), 0);
        assert_eq!(PointerId::touch(0).raw(), 1);
        assert_eq!(PointerId::touch(42).raw(), 43);
        assert_ne!(PointerId::MOUSE, PointerId::touch(0));
    }

    #[test]
    fn two_touches_drag_two_widgets_independently() {
        // Multi-touch first-design invariant: two fingers on two
        // widgets each enter capture lock on their own tag and
        // forward `pointer_move` only to their own widget. Single-
        // target capture (`Option<String>`) would alias here — the
        // R51.38 HashMap substrate makes this work without aliasing.
        let mut router = InputRouter::new();
        let (mut state, (ea, ma), (eb, mb)) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let t1 = PointerId::touch(0);
        let t2 = PointerId::touch(1);
        // Touch 1 lands on slider_a's centre (50, 50).
        router.cursor_moved(t1, 50.0, 50.0, &mut state);
        router.pointer_down(t1, &mut state);
        // Touch 2 lands on slider_b's centre (150, 50).
        router.cursor_moved(t2, 150.0, 50.0, &mut state);
        router.pointer_down(t2, &mut state);
        assert_eq!(router.captured_target(t1), Some("slider_a"));
        assert_eq!(router.captured_target(t2), Some("slider_b"));
        // Drag each in opposite directions. Each widget's
        // `pointer_move` only sees its own touch's coords; the
        // sequence below would alias under a single-target capture
        // implementation (the second touch would overwrite the
        // first's lock).
        router.cursor_moved(t1, 70.0, 50.0, &mut state); // slider_a right
        router.cursor_moved(t2, 130.0, 50.0, &mut state); // slider_b left
        router.pointer_up(t1, &mut state);
        router.pointer_up(t2, &mut state);
        // slider_a saw the click-point + one drag move.
        let log_a = read_moves(&ma);
        assert_eq!(log_a.len(), 2);
        assert!((log_a[0].0 - 0.5).abs() < 1e-4); // click point
        assert!((log_a[1].0 - 0.8333).abs() < 1e-3); // (70-20)/60
        // slider_b saw its own click-point + drag.
        let log_b = read_moves(&mb);
        assert_eq!(log_b.len(), 2);
        assert!((log_b[0].0 - 0.5).abs() < 1e-4); // click point
        assert!((log_b[1].0 - 0.1666).abs() < 1e-3); // (130-120)/60
        // PointerEnter / Down / Up streams independent per widget.
        assert_eq!(
            read(&ea),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(
            read(&eb),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(router.captured_target(t1), None);
        assert_eq!(router.captured_target(t2), None);
    }

    #[test]
    fn mouse_and_touch_dont_alias_hover() {
        // Mouse on slider_a, touch on slider_b — both pointers have
        // their own `hover_target` entry. Per-pointer dispatch means
        // each widget sees its own PointerEnter without aliasing.
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let touch = PointerId::touch(0);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("slider_a"));
        assert_eq!(router.hover_target(touch), Some("slider_b"));
        // Each widget observed exactly one PointerEnter — neither
        // saw the other pointer's transitions.
        assert_eq!(read(&ea), vec!["PointerEnter".to_string()]);
        assert_eq!(read(&eb), vec!["PointerEnter".to_string()]);
    }

    #[test]
    fn releasing_one_touch_does_not_release_other_capture() {
        // Per-pointer capture isolation: lifting one finger must
        // not break the other finger's drag. The shared single-
        // target `Option<String>` capture would collapse here (the
        // first pointer_up would clear the lock for both).
        let mut router = InputRouter::new();
        let (mut state, _a, _b) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let t1 = PointerId::touch(0);
        let t2 = PointerId::touch(1);
        router.cursor_moved(t1, 50.0, 50.0, &mut state);
        router.pointer_down(t1, &mut state);
        router.cursor_moved(t2, 150.0, 50.0, &mut state);
        router.pointer_down(t2, &mut state);
        assert_eq!(router.captured_target(t1), Some("slider_a"));
        assert_eq!(router.captured_target(t2), Some("slider_b"));
        // Lift touch 1 only.
        router.pointer_up(t1, &mut state);
        assert_eq!(router.captured_target(t1), None);
        // Touch 2's lock survives.
        assert_eq!(router.captured_target(t2), Some("slider_b"));
        router.pointer_up(t2, &mut state);
        assert_eq!(router.captured_target(t2), None);
    }

    #[test]
    fn cursor_left_for_one_pointer_keeps_other_state() {
        // Cursor leaves the window for the mouse pointer, but a
        // touch pointer's hover should be untouched. Per-pointer
        // `cursor_left` only drops the matching id's cursor.
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let touch = PointerId::touch(0);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state);
        router.cursor_left(PointerId::MOUSE, &mut state);
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
        assert_eq!(router.hover_target(touch), Some("slider_b"));
        // slider_a saw Enter + Leave; slider_b only Enter.
        assert_eq!(
            read(&ea),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(read(&eb), vec!["PointerEnter".to_string()]);
    }

    #[test]
    fn wants_capture_cache_co_locates_with_hover_walk() {
        // R51.40 §5.35 — `pointer_down` reads a cached bit
        // populated by the matching `refresh_hover` instead of
        // walking the state-scene tree itself. Behavioural
        // verification: a drag-aware widget still enters capture on
        // press, and a button-like widget still does not — same
        // observable outcome as the pre-R51.40 walk-on-click path,
        // exercising the cache lookup chain end-to-end.
        let mut router = InputRouter::new();
        let (mut state, _events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        // Pointer hovers a drag-aware widget — the cache hit makes
        // pointer_down lock capture without re-walking the scene.
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("main_slider"));
        router.pointer_up(PointerId::MOUSE, &mut state);
        // Drop hover (cache cleared on PointerLeave path).
        let mut router2 = InputRouter::new();
        let (mut state2, _events) = state_with_button();
        let paint2 = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router2.update_paint_scene(paint2, &mut state2);
        router2.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state2);
        // Button-like widget cached as wants_capture=false — no
        // lock on press.
        router2.pointer_down(PointerId::MOUSE, &mut state2);
        assert_eq!(router2.captured_target(PointerId::MOUSE), None);
    }

    // ─── R51.42 §5.35 sub-index dispatch fixtures + tests ─────

    /// Paint scene with a single composite hit-target carrying the
    /// `"<primary>#<sub_index>"` tag convention from the R51.41 RFC.
    /// One container, fixed rect — mirrors the per-radio rect a real
    /// `RadioGroup` would lay out for index `sub_index` under primary
    /// `primary`.
    fn paint_with_subindex_tag(
        viewport_w: u32,
        viewport_h: u32,
        rect: Rect,
        primary: &str,
        sub_index: &str,
    ) -> Scene {
        let composite_tag = format!("{primary}#{sub_index}");
        let inner = {
            let mut s = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag(composite_tag)
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut s {
                c.rect = rect;
            }
            s
        };
        let mut root = Scene::Container(
            ContainerNode::new(vec![inner])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    /// State scene with one `CaptureExternal` tagged by the primary
    /// half of the composite hit-target convention (no `'#'`). Tests
    /// inspect `captures` to assert the wire payload the router
    /// forwards through `invoke("send", ...)`.
    fn state_with_primary_external(primary: &str) -> (Scene, Arc<Mutex<Vec<String>>>) {
        let (capture, captures) = CaptureExternal::new();
        let scene = Scene::External(
            ExternalNode::new(Box::new(capture)).with_tag(primary.to_string()),
        );
        (scene, captures)
    }

    /// State scene with one `DragCaptureExternal` tagged by the
    /// primary half. Tests use this to verify the wiring of
    /// `widget_wants_capture` and `forward_pointer_move` through
    /// the sub-index split — even though `RadioGroup` itself returns
    /// `wants_pointer_capture = false`, a future composite drag
    /// widget would rely on the symmetric path landing here.
    fn state_with_primary_drag(primary: &str) -> (Scene, EventLog, MoveLog) {
        let (drag, events, moves) = DragCaptureExternal::new();
        let scene = Scene::External(
            ExternalNode::new(Box::new(drag)).with_tag(primary.to_string()),
        );
        (scene, events, moves)
    }

    /// R738 §5.35 — paint fixture for a composite that paints BOTH a
    /// primary track rect and a sub-tagged thumb rect (the range-slider
    /// shape), so primary-rect vs sub-rect normalization differ.
    fn paint_with_primary_and_subtag(
        viewport_w: u32,
        viewport_h: u32,
        primary_rect: Rect,
        thumb_rect: Rect,
        primary: &str,
        sub_index: &str,
    ) -> Scene {
        let mut thumb = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag(format!("{primary}#{sub_index}"))
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut thumb {
            c.rect = thumb_rect;
        }
        let mut track = Scene::Container(
            ContainerNode::new(vec![thumb])
                .with_tag(primary.to_string())
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut track {
            c.rect = primary_rect;
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![track]).with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    #[test]
    fn capture_normalize_against_primary_uses_track_rect() {
        // R738 regression: a capture widget that opts into
        // `capture_normalize_against_primary` (the dual-thumb range
        // slider) normalizes the dragged cursor against the PRIMARY
        // (track) rect even though capture pinned a thumb sub-tag — so
        // x_rel maps across the whole track instead of saturating on the
        // thumb rect (the bug where grabbing the low thumb moved the
        // high thumb). The `DragCaptureExternal` mock here returns true.
        let mut router = InputRouter::new();
        let (drag, _events, moves) = DragCaptureExternal::with_normalize_primary(true);
        let mut state =
            Scene::External(ExternalNode::new(Box::new(drag)).with_tag("range"));
        // Track x 80..120 (width 40); thumb x 96..104 (width 8). Cursor
        // x=98 is on the thumb but off-centre so the two rects differ.
        let paint = paint_with_primary_and_subtag(
            200,
            200,
            Rect::new(80, 80, 40, 40),
            Rect::new(96, 80, 8, 40),
            "range",
            "low",
        );
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 98.0, 100.0, &mut state); // hover thumb
        router.pointer_down(PointerId::MOUSE, &mut state); // capture range#low + forward
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("range#low"));
        let log = read_moves(&moves);
        assert_eq!(log.len(), 1, "click-to-position forwards once");
        // (98-80)/40 = 0.45 against the TRACK; against the 8px thumb it
        // would be (98-96)/8 = 0.25. Asserting 0.45 proves the opt-in
        // normalizes against the primary rect.
        assert!(
            (log[0].0 - 0.45).abs() < 1e-4,
            "x_rel normalized against the track (0.45), got {}",
            log[0].0
        );
    }

    #[test]
    fn capture_default_normalizes_against_subtag_even_with_primary_painted() {
        // R738 — the DEFAULT (false) normalizes against the captured
        // sub-tag's rect even when the primary IS painted. This is the
        // dock tear-off's exact shape (panel primary + header sub-tag):
        // its tear-off fraction is measured relative to the grabbed
        // header, so it must NOT normalize against the whole panel.
        let mut router = InputRouter::new();
        let (drag, _events, moves) = DragCaptureExternal::with_normalize_primary(false);
        let mut state =
            Scene::External(ExternalNode::new(Box::new(drag)).with_tag("panel"));
        let paint = paint_with_primary_and_subtag(
            200,
            200,
            Rect::new(80, 80, 40, 40),
            Rect::new(96, 80, 8, 40),
            "panel",
            "header",
        );
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 98.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("panel#header"));
        let log = read_moves(&moves);
        assert_eq!(log.len(), 1);
        // (98-96)/8 = 0.25 against the 8px header sub-rect (NOT 0.45,
        // which would be the whole-panel normalization).
        assert!(
            (log[0].0 - 0.25).abs() < 1e-4,
            "x_rel normalized against the sub-tag header (0.25), got {}",
            log[0].0
        );
    }

    #[test]
    fn sub_index_dispatch_forwards_idx_prefixed_event_name() {
        // Paint `main_group#2` + state `main_group` → cursor on the
        // sub-region drives the composite External with the
        // `"2:<EventName>"` wire payload the RadioGroup invoke
        // handler parses (radio_group.rs:357 `split_once(':')`).
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_primary_external("main_group");
        let paint = paint_with_subindex_tag(
            200,
            200,
            Rect::new(80, 80, 40, 40),
            "main_group",
            "2",
        );
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec![
                "2:PointerEnter".to_string(),
                "2:PointerDown".into(),
                "2:PointerUp".into(),
            ],
        );
        // The router stores the raw paint tag (with `#`) in its
        // hover map so subsequent leave-on-stray still routes to
        // the right sub-region.
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_group#2"));
    }

    #[test]
    fn single_tag_backwards_compat() {
        // Plain `main_btn` tag (no `'#'`) routes verbatim — the
        // R51.34 / .37 / .38 / .40 fixtures all use this shape; this
        // test re-asserts the unsplit path under the R51.42 splitter
        // to lock in backwards compatibility.
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
    }

    #[test]
    fn sub_index_capture_wires_to_primary() {
        // Composite drag-aware widget (paint `composite#0`, state
        // `composite` drag-capture). `widget_wants_capture` looks up
        // the primary half and returns `true`; `pointer_down` locks
        // capture on the *raw* paint tag (so the leave-deferred
        // refresh can find the sub-region rect again); the captured
        // `forward_pointer_move` normalises via the raw rect but
        // routes the call to the primary `External`. R51.41
        // composite hit-target convention is symmetric for drag-
        // aware composites even though `RadioGroup` itself opts out.
        // (R738 §5.35 — the dock tear-off relies on this raw-sub-rect
        // normalization: its tear-off fraction is measured relative to
        // the grabbed header, not the whole panel. The range slider,
        // which needs whole-track normalization, instead makes the
        // *track* its sole capture target rather than tagging thumbs.)
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_primary_drag("composite");
        let paint = paint_with_subindex_tag(
            200,
            200,
            Rect::new(80, 80, 40, 40),
            "composite",
            "0",
        );
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        // Captured target stores the raw paint tag (with `#`) so a
        // subsequent stray off the sub-region keeps the drag alive.
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            Some("composite#0"),
        );
        // Click-to-position forwarded one normalised entry — rect is
        // (80, 80, 40, 40) so cursor (100, 100) maps to (0.5, 0.5).
        let log = read_moves(&moves);
        assert_eq!(log.len(), 1);
        assert!((log[0].0 - 0.5).abs() < 1e-4 && (log[0].1 - 0.5).abs() < 1e-4);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
    }

    #[test]
    fn empty_subindex_treated_as_unsplit() {
        // Degenerate paint tag `main_btn#` (trailing `'#'` with empty
        // sub-index). The router must collapse to the unsplit path
        // so the wire payload is `PointerEnter`, not the malformed
        // `:PointerEnter` that composite widgets would reject. The
        // application's paint-tag schema is treated as opaque: the
        // router never panics, and the dispatch payload is well-
        // formed under either convention.
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button(); // tag = "main_btn"
        let paint = {
            // Hand-build a paint scene whose tagged container ends
            // with a literal `'#'` — `paint_with_button` uses the
            // plain tag so we inline the construction here.
            let mut inner = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("main_btn#")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut inner {
                c.rect = Rect::new(80, 80, 40, 40);
            }
            let mut root = Scene::Container(
                ContainerNode::new(vec![inner])
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut root {
                c.rect = Rect::new(0, 0, 200, 200);
            }
            root
        };
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        // Unsplit path: payload is the raw event name, no `'<empty>:'`
        // prefix.
        assert_eq!(
            read(&captures),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
    }

    #[test]
    fn split_subindex_helper_covers_all_shapes() {
        // Pure helper coverage: the dispatch path tests exercise the
        // common shapes end-to-end, but the corner cases (empty
        // primary, multiple `'#'`) deserve their own assertion so a
        // future refactor cannot regress them silently.
        assert_eq!(split_subindex("main_btn"), ("main_btn", None));
        assert_eq!(split_subindex("group#0"), ("group", Some("0")));
        assert_eq!(split_subindex("group#42"), ("group", Some("42")));
        assert_eq!(split_subindex("group#"), ("group", None));
        // Empty primary — state-scene lookup will silently fail, but
        // the split itself is well-defined.
        assert_eq!(split_subindex("#0"), ("", Some("0")));
        // Multiple `'#'` — `split_once` stops at the first; the
        // remainder is opaque to the router (a future schema may
        // give it meaning, e.g. nested sub-indexing).
        assert_eq!(split_subindex("a#b#c"), ("a", Some("b#c")));
    }

    // ─── R51.186 §5.45 R55.C.2 wheel dispatch fixtures + tests ────

    use pinion_core::scene::{BoxNode, ScrollNode};
    use pinion_core::widgets::scroll::ScrollState;
    use std::rc::Rc;

    /// Build a paint scene with a single `Scene::Scroll` wrapping a
    /// `Scene::Box`. Optionally attach a `ScrollState` for wheel
    /// routing — `None` exercises the "declarative-only scroll"
    /// silent-drop path; `Some(rc)` exercises the dispatch path.
    fn paint_with_scroll(
        viewport_w: u32,
        viewport_h: u32,
        scroll_viewport: Rect,
        content_w: u32,
        content_h: u32,
        state: Option<Rc<ScrollState>>,
    ) -> Scene {
        let content = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, content_w, content_h),
            Color::default(),
        ));
        let mut scroll = ScrollNode::new(scroll_viewport, content);
        if let Some(s) = state {
            scroll = scroll.with_state(s);
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![Scene::Scroll(scroll)])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    #[test]
    fn r55_c2_wheel_no_op_without_cursor() {
        // R55.C.2 — wheel before any `cursor_moved` for this
        // pointer (cursor never entered the window) is a silent
        // drop. winit / web / iOS reuse the last stored cursor,
        // so an empty `cursors` map = no dispatch target.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            500,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        // No cursor_moved before wheel.
        let dispatched =
            router.wheel(PointerId::MOUSE, WheelDelta::Pixels { dx: 0.0, dy: 40.0 });
        assert!(!dispatched);
        assert_eq!(state.offset(), (0, 0));
    }

    #[test]
    fn r55_c2_wheel_no_op_without_paint_scene() {
        // R55.C.2 — wheel before the first paint commit is a
        // silent drop. The router has no `last_paint_scene` to
        // hit-test against.
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched =
            router.wheel(PointerId::MOUSE, WheelDelta::Pixels { dx: 0.0, dy: 40.0 });
        assert!(!dispatched);
    }

    #[test]
    fn r55_c2_wheel_no_op_off_scroll_container() {
        // R55.C.2 — cursor on a non-scroll widget (the button
        // fixture) is a silent drop. The button is not a
        // wheel target.
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state_scene);
        let dispatched =
            router.wheel(PointerId::MOUSE, WheelDelta::Pixels { dx: 0.0, dy: 40.0 });
        assert!(!dispatched);
    }

    #[test]
    fn r55_c2_wheel_silent_drop_on_stateless_scroll() {
        // R55.C.2 — the ScrollNode covers the cursor but has no
        // `state` link (declarative-only). The router drops
        // silently rather than panicking — the application can
        // ship a Scroll primitive without wiring input routing.
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            500,
            None,
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched =
            router.wheel(PointerId::MOUSE, WheelDelta::Pixels { dx: 0.0, dy: 40.0 });
        assert!(!dispatched);
    }

    #[test]
    fn r55_c2_wheel_pixels_scrolls_attached_state() {
        // R55.C.2 — Pixels deltas route through verbatim into
        // `ScrollState::scroll_by`. Positive `dy` scrolls
        // downward (W3C sign convention).
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched =
            router.wheel(PointerId::MOUSE, WheelDelta::Pixels { dx: 0.0, dy: 40.0 });
        assert!(dispatched);
        assert_eq!(state.offset(), (0, 40));
        // Second wheel — accumulates.
        router.wheel(PointerId::MOUSE, WheelDelta::Pixels { dx: 0.0, dy: 35.0 });
        assert_eq!(state.offset(), (0, 75));
        // Horizontal axis routes too.
        router.wheel(PointerId::MOUSE, WheelDelta::Pixels { dx: 12.0, dy: 0.0 });
        assert_eq!(state.offset(), (12, 75));
    }

    #[test]
    fn r55_c2_wheel_lines_multiplies_by_line_height_px() {
        // R55.C.2 — Lines deltas scale by `LINE_HEIGHT_PX`
        // (16, the W3C / browser default). One line = 16 px;
        // three lines = 48 px.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched =
            router.wheel(PointerId::MOUSE, WheelDelta::Lines { dx: 0.0, dy: 3.0 });
        assert!(dispatched);
        assert_eq!(state.offset(), (0, 48));
        // Negative line delta scrolls upward; clamped at zero.
        router.wheel(PointerId::MOUSE, WheelDelta::Lines { dx: 0.0, dy: -10.0 });
        assert_eq!(state.offset(), (0, 0));
    }

    #[test]
    fn r55_c2_wheel_clamps_against_state_bounds() {
        // R55.C.2 — overshooting the declared bound clamps at the
        // bound rather than wrapping. ScrollState's own clamp
        // logic carries the policy; the router just feeds the
        // delta through.
        let state = Rc::new(ScrollState::new());
        state.set_max(100, 100);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        router.wheel(PointerId::MOUSE, WheelDelta::Pixels { dx: 0.0, dy: 9999.0 });
        // Bound is 100 on the y axis.
        assert_eq!(state.offset(), (0, 100));
    }

    #[test]
    fn r55_c2_wheel_nan_delta_is_zero_offset() {
        // R55.C.2 — adversarial NaN delta clamps to zero (NaN
        // guard in `round_clamp_i32`); the dispatch still ran
        // (the router resolved a state target), so the return
        // is `true` — backends still consider this a
        // "dispatched" wheel even though the offset did not
        // move. Matches the R51.145 `clamp_frame_dt` NaN guard
        // precedent.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched = router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: f32::NAN, dy: f32::NAN },
        );
        assert!(dispatched, "NaN delta still counts as a dispatched wheel");
        assert_eq!(state.offset(), (0, 0));
    }

    #[test]
    fn r55_c2_wheel_routes_through_last_cursor_position() {
        // R55.C.2 — the router uses the *current* cursor stored
        // for the pointer, so moving the cursor away from the
        // scroll viewport before wheeling silently drops the
        // wheel. Mirrors winit's MouseWheel-without-position
        // contract: the surface owns the cursor; the wheel
        // applies to whatever is under it now.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        // Cursor enters scroll → wheel dispatches.
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        assert!(router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 30.0 },
        ));
        assert_eq!(state.offset(), (0, 30));
        // Cursor leaves the scroll viewport → wheel silently
        // drops. The stored cursor is still set (in fact the
        // refresh fired PointerLeave on the button-like state
        // scene's `main_btn` which is silent under
        // CaptureExternal), but it does not cover any scroll
        // viewport.
        router.cursor_moved(PointerId::MOUSE, 150.0, 150.0, &mut state_scene);
        assert!(!router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 30.0 },
        ));
        // Offset unchanged.
        assert_eq!(state.offset(), (0, 30));
    }

    #[test]
    fn r55_c2_two_pointers_each_route_through_their_own_cursor() {
        // R55.C.2 — multi-pointer: two pointers each track their
        // own cursor; a wheel for `PointerId::touch(0)` reads
        // that pointer's cursor, not the mouse's. The shared
        // `cursors` map keyed by `PointerId` keeps the lookups
        // independent.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        let t = PointerId::touch(0);
        // Mouse cursor over the scroll; touch cursor outside.
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        router.cursor_moved(t, 150.0, 150.0, &mut state_scene);
        // Wheel via touch — touch cursor is outside the scroll
        // viewport → silent drop.
        assert!(!router.wheel(t, WheelDelta::Pixels { dx: 0.0, dy: 20.0 }));
        assert_eq!(state.offset(), (0, 0));
        // Wheel via mouse — dispatches.
        assert!(router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 20.0 },
        ));
        assert_eq!(state.offset(), (0, 20));
    }

    // ─── R51.187 §5.45 R55.C.3 keyboard scroll dispatch tests ─────

    #[test]
    fn r55_c3_arrow_keys_step_one_line_each_axis() {
        // R55.C.3 — arrow keys translate to ±LINE_HEIGHT_PX (16)
        // on the matching axis. Mirrors the W3C `WheelEvent`
        // sign convention positive `dy` = scroll downward.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert_eq!(state.offset(), (0, 16));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert_eq!(state.offset(), (0, 32));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowUp"));
        assert_eq!(state.offset(), (0, 16));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowRight"));
        assert_eq!(state.offset(), (16, 16));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowLeft"));
        assert_eq!(state.offset(), (0, 16));
    }

    #[test]
    fn r55_c3_page_keys_step_one_viewport() {
        // R55.C.3 — PageDown / PageUp step by the scroll
        // container's viewport extent on the matching axis.
        // Viewport height = 100 px → PageDown adds 100, PageUp
        // subtracts 100.
        let state = Rc::new(ScrollState::new());
        state.set_max(1000, 1000);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            2000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        assert!(router.scroll_key(PointerId::MOUSE, "PageDown"));
        assert_eq!(state.offset(), (0, 100));
        assert!(router.scroll_key(PointerId::MOUSE, "PageDown"));
        assert_eq!(state.offset(), (0, 200));
        assert!(router.scroll_key(PointerId::MOUSE, "PageUp"));
        assert_eq!(state.offset(), (0, 100));
    }

    #[test]
    fn r55_c3_home_end_jump_to_y_extremes() {
        // R55.C.3 — Home resets y to 0; End jumps y to max_y.
        // The horizontal offset is preserved (W3C "vertical
        // extreme" semantics — Ctrl-Home / Ctrl-End for corner
        // jumps is R55.C.4 carry).
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 800);
        state.scroll_to(50, 400);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            1000,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        assert!(router.scroll_key(PointerId::MOUSE, "Home"));
        assert_eq!(state.offset(), (50, 0), "Home preserves x, resets y");
        assert!(router.scroll_key(PointerId::MOUSE, "End"));
        assert_eq!(state.offset(), (50, 800), "End preserves x, jumps to max_y");
    }

    #[test]
    fn r55_c3_unknown_key_returns_false() {
        // R55.C.3 — keys not in the recognised set (Tab, Enter,
        // Escape, Space, character keys) return false so the
        // caller's regular `apply_key` arm stays the primary
        // path for widget-bound shortcuts.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        for key in ["Space", "Enter", "Tab", "Escape", "a", "F1"] {
            assert!(
                !router.scroll_key(PointerId::MOUSE, key),
                "unrecognised key {key} must return false",
            );
        }
        assert_eq!(state.offset(), (0, 0), "no key advanced the offset");
    }

    #[test]
    fn r55_c3_no_op_when_cursor_off_scroll() {
        // R55.C.3 — same router-state guard as `wheel`: cursor
        // outside any scroll container is a silent drop. The
        // application's regular `apply_key` arm still runs (the
        // backend's `handle_named_key` fallback fires only after
        // `apply_key` returns unhandled, and `scroll_key`'s false
        // simply lets the key dispatch sequence end without an
        // unintended scroll).
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state_scene);
        // No scroll node under cursor → silent drop on every key.
        for key in ["ArrowDown", "ArrowUp", "PageDown", "PageUp", "Home", "End"] {
            assert!(!router.scroll_key(PointerId::MOUSE, key));
        }
        let _ = state; // unused but kept symmetric with other tests
    }

    #[test]
    fn r55_c3_arrow_clamps_against_bounds() {
        // R55.C.3 — ArrowUp from offset 0 clamps at 0 (lower
        // bound); ArrowDown past max_y clamps at max_y. The
        // ScrollState clamp logic carries the policy.
        let state = Rc::new(ScrollState::new());
        state.set_max(0, 40);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            140,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        // ArrowUp at zero — clamp lower.
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowUp"));
        assert_eq!(state.offset(), (0, 0));
        // Three ArrowDowns at +16 each = 48; clamp at max_y = 40.
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert_eq!(state.offset(), (0, 40));
    }

    #[test]
    fn r55_c2_line_height_px_constant_is_w3c_default() {
        // R55.C.2 — pin the constant value so a future override
        // (R55.C.4 per-widget line-height) shows up as an
        // explicit test edit, not a silent regression.
        assert!((LINE_HEIGHT_PX - 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn update_paint_scene_refreshes_every_active_pointer() {
        // After a layout change, every active pointer's hover_target
        // must re-resolve. With two pointers active (mouse + touch),
        // both should observe the layout shift independently.
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        router.update_paint_scene(paint_with_two_sliders(200, 200), &mut state);
        let touch = PointerId::touch(0);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state);
        // Now repaint with both sliders shifted out from under both
        // cursors. paint_with_two_sliders uses fixed rects; build a
        // bare root with no children to simulate "both widgets
        // moved away".
        let bare_root = Scene::Container(
            ContainerNode::new(vec![])
                .with_style(BoxStyle::filled(Color::default())),
        );
        router.update_paint_scene(bare_root, &mut state);
        // Both pointers lost their hover — each sees PointerLeave.
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
        assert_eq!(router.hover_target(touch), None);
        assert_eq!(
            read(&ea),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(
            read(&eb),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
    }

    // ─── R664 §5.49 W3C double-click detection ─────────────────────

    /// Two consecutive presses with no cursor move in between (the
    /// `DeferredInput::DoubleClick` drain shape) fire one synthetic
    /// `DoubleClick` named event on top of the second `PointerDown`.
    /// The first press only emits `PointerDown` — `DoubleClick`
    /// requires the prior `last_press` snapshot.
    #[test]
    fn r664_back_to_back_presses_emit_double_click_event() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        // First click cycle: PointerEnter + PointerDown + PointerUp.
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        // Second press at the same coord — well inside W3C 300ms / 5px.
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
                "PointerDown".into(),
                "DoubleClick".into(),
                "PointerUp".into(),
            ],
        );
    }

    /// Position delta exceeding [`DOUBLE_CLICK_DIST_PX`] between the
    /// two presses disqualifies the W3C `dblclick` heuristic — the
    /// second press is a normal `PointerDown` with no synthetic
    /// `DoubleClick`. 10 px on the same widget is enough; the Material
    /// 3 / Cocoa convention rejects 6 px+.
    #[test]
    fn r664_spatially_separated_presses_do_not_double_click() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        // Cursor moves 10 px before second press — over the 5 px window.
        router.cursor_moved(PointerId::MOUSE, 110.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(
            !read(&captures).iter().any(|s| s == "DoubleClick"),
            "second press 10 px away must not double-click",
        );
    }

    /// A press on widget A then a press on widget B does not produce
    /// `DoubleClick` even if temporally adjacent — the `last_press`
    /// guard tracks the target tag, not just the timestamp. Mirror of
    /// the W3C `target` equality requirement in the `dblclick`
    /// dispatch contract.
    #[test]
    fn r664_cross_target_presses_do_not_double_click() {
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        router.update_paint_scene(paint_with_two_sliders(200, 200), &mut state);
        let touch = PointerId::touch(0);
        // Single pointer crosses widgets — same PointerId.
        router.cursor_moved(touch, 50.0, 50.0, &mut state); // on slider A
        router.pointer_down(touch, &mut state);
        router.pointer_up(touch, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state); // on slider B
        router.pointer_down(touch, &mut state);
        router.pointer_up(touch, &mut state);
        assert!(
            !read(&ea).iter().any(|s| s == "DoubleClick"),
            "slider A press → release must not see DoubleClick after target switch",
        );
        assert!(
            !read(&eb).iter().any(|s| s == "DoubleClick"),
            "slider B first press must not see stale DoubleClick from slider A",
        );
    }

    /// Triple-click resets after detail=2 — the third press starts a
    /// fresh single/double cycle. The 4th press completes another
    /// double-click. Confirms the substrate pins binary single/double
    /// per `[[abstraction-needs-second-consumer]]` until a triple
    /// consumer surfaces.
    #[test]
    fn r664_triple_press_resets_after_double() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        // 1st press — single. 2nd press — double. 3rd press — single
        // again (no triple). 4th press — double again.
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&captures);
        let double_count = log.iter().filter(|s| s.as_str() == "DoubleClick").count();
        assert_eq!(double_count, 2, "exactly two DoubleClick fires across 4 presses");
    }
}
