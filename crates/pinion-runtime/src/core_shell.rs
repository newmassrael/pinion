//! R51.122 §5.41 — backend-agnostic dispatch substrate [`CoreShell<V>`].
//!
//! Sibling lift of `pinion_shell::ShellCore` (R51.92 visibility) and
//! `pinion_tui::ShellCoreTui` (R51.117 extraction).
//!
//! The four pieces of state every backend's dispatch loop needs
//! (`scene`, `cached_state`, `router`, `intent_queue`) live here in
//! `pinion-runtime` so any future backend (mobile, RPC-only, second
//! TUI library, native `AppKit`) reaches the same substrate without
//! duplicating the `Scene` plus [`InputRouter`] plus [`IntentQueue`]
//! plumbing.
//!
//! ## Why now (4-round split, R51.122-R51.125)
//!
//! R51.117 landed `ShellCoreTui` as the second backend's substrate
//! struct (first cut). The two substrate structs duplicated about
//! 70% of their dispatch methods (`forward`, `apply_key`,
//! `cursor_moved`, `pointer_down`, `pointer_up`, `touch_event`, plus
//! the drain + refresh tail). The only difference was where the
//! backend-specific state lived: the Vello side carried focus,
//! modifiers, text cache, previews, revision, last paint, AT caches,
//! and redraw flag; the TUI side carried the log sink. The
//! `substrate-incompleteness-signal` cycle the project documents
//! triggers on the second-client overlap, and R51.122 is the first
//! round of the 4-round lift.
//!
//! - **R51.122 (this round)** — `CoreShell<V: WidgetCore>` lands in
//!   `pinion-runtime` with the four backend-agnostic fields + the
//!   `DispatchTail<S>` return shape (intents + optional
//!   `state_change`). Pure substrate — no logging, no redraw flag, no
//!   backend wrapping yet (R51.123 / R51.124 land the two wrappers).
//! - **R51.123** — `pinion_shell::ShellCore` reduces to
//!   `core: CoreShell<V>` + the Vello-specific extras (focus /
//!   modifiers / `text_cache` / previews / revision / `last_paint_layout`
//!   / `last_access_*` / `redraw_requested`). Existing dispatch
//!   methods forward to `core` + log + bookkeep.
//! - **R51.124** — `pinion_tui::ShellCoreTui` reduces to
//!   `core: CoreShell<V>` + the TUI-specific `log_sink`.
//!   `refresh_state` becomes a thin wrapper over `core.tail()` +
//!   `log_sink` routing.
//! - **R51.125** — `dispatch_rpc` lifts to a `ShellDispatch` trait
//!   (declared here in `pinion-runtime`, impl'd in `pinion-shell`)
//!   so the `pinion-rpc → pinion-runtime` direction stays free of
//!   any reverse crate dep.
//!
//! ## Dep direction
//!
//! `pinion-runtime` already depends on `pinion-core` (where
//! [`WidgetCore`] lives after R51.121) + `pinion-text` (text shaping
//! primitive). The lift adds no new crate deps — `CoreShell<V>`
//! reuses the existing [`InputRouter`] / [`IntentQueue`] /
//! [`walk_scene_and_drain`] from this crate's `input` / `intent_queue`
//! modules. Critically, the lift does NOT introduce a
//! `pinion-runtime → pinion-a11y` or `→ pinion-rpc` direction: AT
//! caches stay in `pinion-shell::ShellCore`, the RPC dispatcher stays
//! at the Vello backend (R51.125 trait extraction preserves the
//! topology).
//!
//! ## §6.3 view-fn purity preserved
//!
//! [`CoreShell`] never invokes the view fn directly — backends
//! compute their paint scene with `V::view(state, &frame)` + their
//! own layout pass (Vello: `compute_layout` against `text_cache`;
//! TUI: no layout, direct grapheme-cell mapping) and feed the result
//! back through [`CoreShell::update_paint_scene`] so the router's
//! hit-test snapshot refreshes. The view fn stays a pure
//! `Fn(state, &Frame) -> Scene` per §6.3 R51.27 `dry_run` invariant.

use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::scene::ExternalNode;
use pinion_core::{Scene, WidgetCore};

use crate::input::{InputRouter, PointerId, Touch, TouchPhase};
use crate::intent_queue::{walk_scene_and_drain, IntentQueue};

/// R51.122 §5.41 — backend-agnostic dispatch substrate.
///
/// Generic over any [`WidgetCore`]-implementing binding. Owns the
/// four pieces of state every backend's dispatch loop needs:
///
/// - [`Scene`] — the authoritative state scene carrying the SCXML
///   widget through `Scene::External`.
/// - `V::State` — cached projection of the live state, refreshed on
///   every dispatch tail by [`WidgetCore::read_state`].
/// - [`InputRouter`] — winit-free pointer routing primitive
///   (R48 §5.35 + R51.108 §5.41 lift). Resolves cursor coords against
///   the most recent paint scene to dispatch `PointerEnter` /
///   `PointerLeave` / `PointerDown` / `PointerUp` to the matching
///   `Scene::External`.
/// - [`IntentQueue`] — per-event harvest buffer the §5.20 walk drains
///   into; returned to callers via [`DispatchTail::intents`].
///
/// All four are private — accessors expose only the read-only shape
/// the surface needs ([`Self::scene`], [`Self::cached_state`]);
/// mutation flows through the dispatch methods. The TUI / Vello /
/// future backends compose `CoreShell<V>` as an inner field rather
/// than inheriting from it (composition-over-inheritance per the
/// supertrait split R51.121 ratified for the widget binding side).
///
/// ## What stays on each backend
///
/// - Vello (`pinion_shell::ShellCore`): focus manager, modifier
///   cache, text layout cache, RPC preview ledger, OCC revision
///   token, last paint layout snapshot, AccessKit emit caches,
///   redraw-requested flag.
/// - TUI (`pinion_tui::ShellCoreTui`): optional `log_sink` for
///   intent + state-change trace lines (silent default per the
///   R51.120 alternate-screen anti-pattern).
pub struct CoreShell<V: WidgetCore> {
    scene: Scene,
    cached_state: V::State,
    router: InputRouter,
    intent_queue: IntentQueue,
}

/// R51.122 §5.41 — post-dispatch bookkeeping artifact returned by
/// every [`CoreShell`] dispatch method.
///
/// Carries the two pieces of information backends use to drive their
/// per-event side effects:
///
/// - `intents` — every §5.20 [`Intent`] the post-dispatch walk
///   drained from the scene's `Scene::External` nodes. Backends log
///   (Vello: `eprintln!`, TUI: optional file sink) and may also
///   forward to a pending-intents observer the RPC `scene/intents`
///   method drains separately (the queue is single-consumer per
///   §5.20; whoever harvests first wins).
/// - `state_change` — `Some(StateChange { before, after })` when the
///   post-dispatch [`WidgetCore::read_state`] noticed a transition
///   from the previous cached state; `None` when the visible state
///   stayed the same. Backends use this to trigger a repaint (Vello:
///   `request_redraw` flag; TUI: caller-side repaint commit on
///   visible change).
///
/// The struct is owned (`Vec` + `Option`) so callers can drain
/// without double-borrowing the `CoreShell`. Returning by value is
/// cheap — the intent vec usually has zero or one element and
/// `V::State` is `Copy` per the [`WidgetCore::State`] trait bound.
#[derive(Debug)]
pub struct DispatchTail<S> {
    /// Intents drained by the §5.20 walk after the dispatch arm ran.
    /// Empty on most events — only widget-event-emitting dispatch
    /// arms (`forward` + `apply_key` on accepted keys + pointer click
    /// / touch tap cycles) produce intents.
    pub intents: Vec<Intent>,

    /// `Some(_)` when the cached state actually changed between the
    /// pre- and post-dispatch [`WidgetCore::read_state`] readings;
    /// `None` when the dispatch left the visible state unchanged
    /// (e.g. mouse moves outside any widget, internal SCXML
    /// transitions that emit intents without flipping state).
    pub state_change: Option<StateChange<S>>,
}

impl<S> DispatchTail<S> {
    /// `true` when the tail had no observable effect: no intents
    /// drained, no state transition. Backends that paint on visible
    /// change only skip the repaint commit on `is_empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.intents.is_empty() && self.state_change.is_none()
    }
}

/// R51.122 §5.41 — typed `before` / `after` pair returned inside
/// [`DispatchTail::state_change`].
///
/// `Copy` because the field type `S` is `V::State`, which is `Copy`
/// per [`WidgetCore::State`]'s trait bound (the cached state needs
/// to move freely between the substrate's bookkeeping fields and
/// the paint closure without lifetime gymnastics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateChange<S> {
    /// Cached state immediately before the dispatch arm ran.
    pub before: S,
    /// Cached state immediately after the dispatch arm's post-tail
    /// [`WidgetCore::read_state`] reading.
    pub after: S,
}

impl<V: WidgetCore> Default for CoreShell<V> {
    /// Equivalent to [`Self::new`]; provided so the substrate
    /// composes with any future builder that defaults a member
    /// through [`Default::default`] (workspace lints set
    /// `clippy::pedantic = "deny"` which promotes
    /// `clippy::new_without_default` to a hard build error; this
    /// impl is mandatory).
    fn default() -> Self {
        Self::new()
    }
}

impl<V: WidgetCore> CoreShell<V> {
    /// R51.122 §5.41 — construct a fresh substrate around the
    /// binding's [`WidgetCore::create_external`] SCXML widget.
    ///
    /// The first [`WidgetCore::read_state`] runs synchronously against
    /// the constructed scene so the cached state is correct before
    /// the substrate enters the event loop. The router starts with
    /// no retained paint scene — backends must call
    /// [`Self::update_paint_scene`] after the initial paint to seed
    /// the hit-test snapshot before the first pointer event arrives.
    #[must_use]
    pub fn new() -> Self {
        let scene = Scene::External(
            ExternalNode::new(V::create_external()).with_tag(V::tag()),
        );
        let cached_state = V::read_state(&scene);
        Self {
            scene,
            cached_state,
            router: InputRouter::new(),
            intent_queue: IntentQueue::new(),
        }
    }

    /// Read-only borrow of the authoritative state scene. Tests
    /// reach the widget External through
    /// `Scene::External(node) => &node.handle` when verifying
    /// introspect side effects.
    #[must_use]
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Mutable borrow of the authoritative state scene. Backends
    /// that need to invoke `intervene` / `query` on a specific path
    /// inside the scene (the Vello shell's AT-action dispatch +
    /// `apply_a11y_key` chain) reach in through this accessor; the
    /// standard dispatch methods on this struct cover the common
    /// cases without exposing the scene mutably.
    #[must_use]
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// Read-only borrow of the cached state projection. Backends
    /// pass `*c.cached_state()` to their view fn at paint time so
    /// the next frame reflects the just-dispatched transition.
    #[must_use]
    pub fn cached_state(&self) -> &V::State {
        &self.cached_state
    }

    /// R51.122 §5.41 — hand a freshly-painted scene to the
    /// [`InputRouter`] so the next pointer event resolves against the
    /// visible layout. Both backends call this once per paint commit
    /// (initial + post-state-change + resize repaint).
    pub fn update_paint_scene(&mut self, paint_scene: Scene) {
        self.router.update_paint_scene(paint_scene, &mut self.scene);
    }

    /// R51.122 §5.41 — read-only proxy to the underlying
    /// [`InputRouter::hover_target`]. Backends use this to read the
    /// current hover target for `click_to_focus` style follow-up
    /// (the Vello shell's W3C HTML-style "press on focusable widget
    /// focuses it; press on background blurs" rule), without
    /// exposing the router's mutable interior.
    #[must_use]
    pub fn hover_target(&self, pid: PointerId) -> Option<&str> {
        self.router.hover_target(pid)
    }

    /// R51.122 §5.41 — drain the post-dispatch bookkeeping artifacts
    /// (intents + optional state change) without running any input
    /// dispatch arm.
    ///
    /// Mostly internal — every dispatch method on this struct calls
    /// `tail()` as its last step. Exposed `pub` for backends that
    /// want to drain outside the dispatch path (e.g. an initial
    /// post-construction drain to surface intents the widget armed
    /// at construction time).
    pub fn tail(&mut self) -> DispatchTail<V::State> {
        walk_scene_and_drain(&mut self.scene, &mut self.intent_queue);
        let intents = self.intent_queue.drain();
        let now = V::read_state(&self.scene);
        let state_change = if now == self.cached_state {
            None
        } else {
            let before = self.cached_state;
            self.cached_state = now;
            Some(StateChange { before, after: now })
        };
        DispatchTail { intents, state_change }
    }

    /// R51.122 §5.41 — translate a typed widget event into the
    /// symbolic `invoke("send", Text(<name>))` call on the scene's
    /// `Scene::External`, then drain the dispatch tail.
    ///
    /// Mirrors the pre-lift `pinion_shell::ShellCore::forward` +
    /// `pinion_tui::ShellCoreTui::forward_event` shape. The `invoke`
    /// `Result` is ignored (statechart-side rejection is a valid
    /// SCXML outcome per the conservative-bump policy); the OCC
    /// revision bump that the Vello shell applied after `forward`
    /// stays in the Vello wrapper because the revision token is
    /// Shell-specific.
    pub fn forward(&mut self, event: V::Event) -> DispatchTail<V::State> {
        let name = V::event_name(event);
        if let Scene::External(node) = &mut self.scene
            && let Some(intro) = node.handle.introspect_mut()
        {
            let _ = intro.invoke("send", IntrospectValue::Text(name.to_string()));
        }
        self.tail()
    }

    /// R51.122 §5.41 — route a key string through
    /// [`WidgetCore::apply_key`]. Returns `Some(DispatchTail)` on
    /// handled (`true` from `apply_key`), `None` on unhandled — the
    /// shell wrapper checks the `Option` to decide whether to bump
    /// any backend-specific bookkeeping (Vello: revision + redraw;
    /// TUI: repaint trigger).
    ///
    /// `focused` carries the focus manager's currently-focused tag —
    /// the Vello shell passes `self.focus.focused()`; the TUI shell
    /// (single-widget today) passes `Some(V::tag())`.
    pub fn apply_key(
        &mut self,
        focused: Option<&str>,
        key: &str,
    ) -> Option<DispatchTail<V::State>> {
        if V::apply_key(&mut self.scene, focused, key) {
            Some(self.tail())
        } else {
            None
        }
    }

    /// R51.122 §5.41 — pointer cursor-move dispatch (cell→pixel or
    /// `winit` → pixel conversion happens at the backend boundary).
    /// Forwards through the [`InputRouter`] then drains the dispatch
    /// tail.
    pub fn cursor_moved(
        &mut self,
        pid: PointerId,
        x: f64,
        y: f64,
    ) -> DispatchTail<V::State> {
        self.router.cursor_moved(pid, x, y, &mut self.scene);
        self.tail()
    }

    /// R51.122 §5.41 — pointer leaves the surface for `pid` (winit's
    /// `CursorLeft`). Drops the cursor + rolls back any in-flight
    /// `Hover`.
    pub fn cursor_left(&mut self, pid: PointerId) -> DispatchTail<V::State> {
        self.router.cursor_left(pid, &mut self.scene);
        self.tail()
    }

    /// R51.122 §5.41 — pointer press (mouse left button down / touch
    /// start). Dispatches `PointerDown` to the current hover target
    /// then drains the dispatch tail. The Vello shell follows up with
    /// its `click_to_focus` step; the substrate stays focus-agnostic.
    pub fn pointer_down(&mut self, pid: PointerId) -> DispatchTail<V::State> {
        self.router.pointer_down(pid, &mut self.scene);
        self.tail()
    }

    /// R51.122 §5.41 — pointer release (mouse left button up / touch
    /// end). Dispatches `PointerUp` to the current hover target then
    /// drains the dispatch tail.
    pub fn pointer_up(&mut self, pid: PointerId) -> DispatchTail<V::State> {
        self.router.pointer_up(pid, &mut self.scene);
        self.tail()
    }

    /// R51.122 §5.41 — pointer cancellation (touch interrupted by OS
    /// gesture, phone-call notification, 4-finger swipe).
    /// Dispatches `PointerCancel` (not `PointerUp`) so the widget
    /// statechart routes `Pressed → Idle` without raising the
    /// activate event; then drains the dispatch tail.
    pub fn pointer_cancel(&mut self, pid: PointerId) -> DispatchTail<V::State> {
        self.router.pointer_cancel(pid, &mut self.scene);
        self.tail()
    }

    /// R51.122 §5.41 — touch event dispatch. Per-finger
    /// [`PointerId::touch(touch.id)`] (so two simultaneous touches
    /// drive two widgets without aliasing the capture lock). Phase
    /// routing matches the pre-lift
    /// `pinion_shell::ShellCore::handle_touch`:
    ///
    /// - [`TouchPhase::Started`] — synthetic
    ///   [`InputRouter::cursor_moved`] to resolve the hover target
    ///   under the press point, then [`InputRouter::pointer_down`].
    /// - [`TouchPhase::Moved`] — [`InputRouter::cursor_moved`] to
    ///   the new position.
    /// - [`TouchPhase::Ended`] — [`InputRouter::pointer_up`] then
    ///   [`InputRouter::cursor_left`] (the next touch with the same
    ///   finger id is a fresh gesture per winit's contract).
    /// - [`TouchPhase::Cancelled`] —
    ///   [`InputRouter::pointer_cancel`] then
    ///   [`InputRouter::cursor_left`] (R51.93 §5.35 lesson:
    ///   cancellation must not raise the activate event the SCXML
    ///   guards on `pointer_up`).
    ///
    /// [`TouchPhase`] is `#[non_exhaustive]` for cross-crate
    /// SemVer-minor variant additions (§5.13 hedge precedent), but
    /// from inside `pinion-runtime` the match is exhaustive — adding
    /// a new variant in this crate would intentionally break this
    /// arm at compile time so the dispatch decision is made
    /// explicit. Cross-crate consumers wrap [`Touch`] in their own
    /// adapters that fall through unknown phases as no-ops.
    pub fn touch_event(&mut self, touch: Touch) -> DispatchTail<V::State> {
        let pid = PointerId::touch(touch.id);
        match touch.phase {
            TouchPhase::Started => {
                self.router.cursor_moved(pid, touch.x, touch.y, &mut self.scene);
                self.router.pointer_down(pid, &mut self.scene);
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
        }
        self.tail()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::test_fixtures::ButtonFixture as TestButton;
    use pinion_core::widgets::button::{ButtonEvent, ButtonState};
    use pinion_core::Frame;

    #[test]
    fn constructor_seeds_cached_state_from_introspect() {
        // R51.122 — fresh substrate reads the Button's initial Idle
        // state via the §5.15 introspect channel inside `new()`.
        let core: CoreShell<TestButton> = CoreShell::new();
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn default_construction_equivalent_to_new() {
        // R51.122 — `CoreShell::default()` mirrors `new()` so tests
        // that need a no-arg constructor can use either.
        let a: CoreShell<TestButton> = CoreShell::default();
        let b: CoreShell<TestButton> = CoreShell::new();
        assert_eq!(a.cached_state(), b.cached_state());
    }

    #[test]
    fn idle_substrate_tail_is_empty() {
        // R51.122 — `tail()` against a fresh substrate (no
        // dispatch ran, no External intent armed at construction)
        // returns an empty `DispatchTail`.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let tail = core.tail();
        assert!(tail.is_empty());
        assert!(tail.intents.is_empty());
        assert!(tail.state_change.is_none());
    }

    #[test]
    fn forward_emits_click_intent_on_keyboard_activate() {
        // R51.122 — `forward(KeyboardActivate)` routes through
        // `invoke("send", Text("KeyboardActivate"))` to the SCXML;
        // the Button's internal transition emits the `click`
        // intent without flipping the visible state (Idle → Idle).
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let tail = core.forward(ButtonEvent::KeyboardActivate);
        assert_eq!(tail.intents.len(), 1, "click intent must drain");
        assert_eq!(tail.intents[0].tag_str(), "test_btn.click");
        assert!(
            tail.state_change.is_none(),
            "KeyboardActivate is an internal transition; visible state unchanged",
        );
    }

    #[test]
    fn apply_key_returns_none_for_unhandled_key() {
        // R51.122 — `apply_key` returns `None` when
        // `WidgetCore::apply_key` reports `false`. ArrowLeft is not
        // a Button keybinding and `apply_aria_activate` rejects it.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        assert!(core.apply_key(Some("test_btn"), "ArrowLeft").is_none());
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn apply_key_returns_tail_with_intent_for_handled_space() {
        // R51.122 — `apply_key(Some(tag), "Space")` resolves through
        // `apply_aria_activate` for the matching focused tag,
        // emitting a `click` intent. State stays Idle (KeyboardActivate
        // is an internal SCXML transition).
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let Some(tail) = core.apply_key(Some("test_btn"), "Space") else {
            panic!("apply_key must return Some for handled Space");
        };
        assert_eq!(tail.intents.len(), 1);
        assert_eq!(tail.intents[0].tag_str(), "test_btn.click");
    }

    #[test]
    fn apply_key_with_wrong_focus_returns_none() {
        // R51.122 — `apply_aria_activate` requires `focused ==
        // Some(tag)`; a foreign tag drops the key with no SCXML
        // dispatch. Substrate observes this as `None` and the
        // backend wrapper skips its post-handle bookkeeping.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        assert!(core.apply_key(Some("other_widget"), "Space").is_none());
        assert!(core.apply_key(None, "Space").is_none());
    }

    #[test]
    fn pointer_cycle_lands_in_hover_with_visible_state_changes() {
        // R51.122 — full click cycle on the test_btn rect:
        //   cursor_moved into rect → Idle → Hover (state changed)
        //   pointer_down            → Hover → Pressed (state changed)
        //   pointer_up              → Pressed → Hover (state changed +
        //                              click intent drained)
        // Each step's `DispatchTail::state_change` carries the
        // before / after pair the backend logs.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        // Seed the router's paint scene so the hit-test resolves.
        let paint = <TestButton as WidgetCore>::view(*core.cached_state(), &Frame::new());
        core.update_paint_scene(paint);

        let t = core.cursor_moved(PointerId::MOUSE, 8.0, 8.0);
        assert_eq!(
            t.state_change.expect("Idle → Hover").after,
            ButtonState::Hover,
        );
        assert!(t.intents.is_empty(), "hover transition emits no intent");

        let t = core.pointer_down(PointerId::MOUSE);
        assert_eq!(
            t.state_change.expect("Hover → Pressed").after,
            ButtonState::Pressed,
        );

        let t = core.pointer_up(PointerId::MOUSE);
        let sc = t.state_change.expect("Pressed → Hover");
        assert_eq!(sc.before, ButtonState::Pressed);
        assert_eq!(sc.after, ButtonState::Hover);
        assert_eq!(t.intents.len(), 1, "Pressed → Hover emits click");
        assert_eq!(t.intents[0].tag_str(), "test_btn.click");
    }

    #[test]
    fn cursor_left_rolls_back_in_flight_hover() {
        // R51.122 — once hovering, `cursor_left` drops the cursor +
        // rolls back the Hover state to Idle. No intent on rollback.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let paint = <TestButton as WidgetCore>::view(*core.cached_state(), &Frame::new());
        core.update_paint_scene(paint);

        let _ = core.cursor_moved(PointerId::MOUSE, 8.0, 8.0);
        assert_eq!(*core.cached_state(), ButtonState::Hover);

        let t = core.cursor_left(PointerId::MOUSE);
        assert_eq!(
            t.state_change.expect("Hover → Idle on cursor_left").after,
            ButtonState::Idle,
        );
        assert!(t.intents.is_empty(), "cursor_left emits no intent");
    }

    #[test]
    fn touch_started_then_ended_runs_full_click_cycle() {
        // R51.122 — touch event phases drive the same SCXML path as
        // mouse: Started seeds hover + presses; Ended releases +
        // drops the cursor. The Pressed → Hover transition fires the
        // `click` intent; the trailing cursor_left then rolls back
        // to Idle.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let paint = <TestButton as WidgetCore>::view(*core.cached_state(), &Frame::new());
        core.update_paint_scene(paint);

        let _ = core.touch_event(Touch {
            id: 0,
            phase: TouchPhase::Started,
            x: 8.0,
            y: 8.0,
        });
        assert_eq!(*core.cached_state(), ButtonState::Pressed);

        let t = core.touch_event(Touch {
            id: 0,
            phase: TouchPhase::Ended,
            x: 8.0,
            y: 8.0,
        });
        // After `pointer_up` then `cursor_left`: Pressed → Hover →
        // Idle. The tail's `state_change` carries only the final
        // delta (the substrate refreshes once per `tail` call).
        assert_eq!(
            t.state_change.expect("Pressed → Idle after Ended").after,
            ButtonState::Idle,
        );
        assert_eq!(t.intents.len(), 1, "click intent on press → release");
        assert_eq!(t.intents[0].tag_str(), "test_btn.click");
    }

    #[test]
    fn touch_cancelled_does_not_fire_click() {
        // R51.122 §5.13 R51.93 — touch cancellation must not raise
        // the activate event the SCXML guards on `pointer_up`.
        // Started → Pressed; Cancelled → Idle without click intent.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let paint = <TestButton as WidgetCore>::view(*core.cached_state(), &Frame::new());
        core.update_paint_scene(paint);

        let _ = core.touch_event(Touch {
            id: 0,
            phase: TouchPhase::Started,
            x: 8.0,
            y: 8.0,
        });
        assert_eq!(*core.cached_state(), ButtonState::Pressed);

        let t = core.touch_event(Touch {
            id: 0,
            phase: TouchPhase::Cancelled,
            x: 8.0,
            y: 8.0,
        });
        assert_eq!(
            t.state_change.expect("Pressed → Idle on cancel").after,
            ButtonState::Idle,
        );
        assert!(t.intents.is_empty(), "cancellation must not fire click");
    }

    #[test]
    fn keybinding_disable_then_enable_routes_through_forward() {
        // R51.122 — typed event forwarding flips Button state via the
        // SCXML `Disable` / `Enable` events. Each transition is
        // observable through `DispatchTail::state_change`.
        let mut core: CoreShell<TestButton> = CoreShell::new();

        let t = core.forward(ButtonEvent::Disable);
        let sc = t.state_change.expect("Idle → Disabled");
        assert_eq!(sc.before, ButtonState::Idle);
        assert_eq!(sc.after, ButtonState::Disabled);

        let t = core.forward(ButtonEvent::Enable);
        let sc = t.state_change.expect("Disabled → Idle");
        assert_eq!(sc.before, ButtonState::Disabled);
        assert_eq!(sc.after, ButtonState::Idle);
    }

    #[test]
    fn update_paint_scene_refreshes_router_hit_test() {
        // R51.122 — without a paint scene the router has no rect to
        // hit-test against; cursor_moved finds no widget and state
        // stays Idle. After `update_paint_scene` the same cursor
        // coord lands on the rect → Hover transition fires.
        let mut core: CoreShell<TestButton> = CoreShell::new();

        let t = core.cursor_moved(PointerId::MOUSE, 8.0, 8.0);
        assert!(
            t.state_change.is_none(),
            "no paint scene → no hover transition",
        );
        assert_eq!(*core.cached_state(), ButtonState::Idle);

        let paint = <TestButton as WidgetCore>::view(*core.cached_state(), &Frame::new());
        core.update_paint_scene(paint);

        let t = core.cursor_moved(PointerId::MOUSE, 8.0, 8.0);
        assert_eq!(
            t.state_change.expect("Idle → Hover after paint").after,
            ButtonState::Hover,
        );
    }
}
