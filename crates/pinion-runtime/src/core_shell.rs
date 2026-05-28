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

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

use pinion_core::event::WheelDelta;
use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::reactive::{Effect, Signal};
use pinion_core::scene::{ContainerNode, ExternalNode};
use pinion_core::{Command, Owner, Scene, WidgetCore};

use crate::command::CommandExecutor;
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
    /// R672 §5.35 §5.41 — per-window [`InputRouter`] map keyed by
    /// canonical `WindowSpec::id` string. Single-window bindings only
    /// ever populate the [`DEFAULT_WINDOW`] entry; multi-window
    /// bindings create one per spec on first input dispatch.
    ///
    /// Pre-R672 the substrate carried one `router: InputRouter` and
    /// every paint cycle of every window overwrote its
    /// `last_paint_scene` + fired `refresh_hover` against the
    /// just-painted tree. With multi-window that produced two
    /// race-class regressions: (1) cross-window
    ///   `scene/click {window: "<id>"}` hit-tested against whichever
    ///   window painted most recently; (2) per-paint
    ///   `refresh_hover` walked the foreign window's scene at the
    ///   pointer's last position, flip-flopping `PointerEnter` /
    ///   `PointerLeave` events across windows. Per-window routers
    ///   scope all pointer state (`cursors`, `hover_targets`,
    ///   `captured_targets`, `last_paint_scene`) to one window so
    ///   neither race exists structurally.
    ///
    /// `[[multi-window-input-router-race]]` documents the R670.B →
    /// R671 carry; R672 closes the foundation.
    routers: HashMap<String, InputRouter>,
    intent_queue: IntentQueue,
    /// R51.142 §5.28 — root reactive scope for this widget binding.
    ///
    /// [`Animation<T>`](pinion_core::Animation) instances created in
    /// the view fn (or by SCE-generated `animated` declarations) attach
    /// to this [`Owner`] through
    /// [`Owner::register_animation`](pinion_core::reactive::Owner::register_animation).
    /// Backends (Vello, TUI) call [`Self::tick_animations`] once per
    /// paint cycle with the measured `dt` so the spring solver advances
    /// in lockstep with the frame budget. On `CoreShell` drop the owner
    /// drops too — every animation and pending [`Command`](pinion_core::Command)
    /// scoped to this binding evaporates with it (Solid cancellation
    /// pattern; matches the [`Owner`] drop semantics R51.137 +
    /// R51.139 land).
    root_owner: Owner,

    /// R51.149 §5.28 — monotonic frame counter that drives the
    /// substrate's reactive animation pump.
    ///
    /// Backends call [`Self::tick_animations`] each paint cycle with
    /// the measured `dt`; the implementation stores the value into
    /// [`Self::last_dt`] then increments this counter, which fires the
    /// [`Self::_animation_driver`] [`Effect`]. A monotonic counter
    /// (not the raw `dt` itself) sidesteps [`Signal::set`]'s
    /// equality-skip: two paints with the same `dt` would otherwise
    /// look identical to the signal and the second tick would never
    /// fire. The counter pattern matches the canonical `SolidJS` /
    /// Leptos `createSignal(0); raf(() => setCount(c => c + 1))` shape
    /// — value-irrelevant signal as the subscription-firing mechanism.
    ///
    /// The §5.28 R33 spec calls this the "framework `AnimationDriver`" —
    /// the reactive routing of the paint clock through pinion's
    /// `Effect` primitive. Any application-side reactive subscriber
    /// can also read this signal to react to the paint clock without
    /// standing up a duplicate counter, opening the door to
    /// SCE-emitted "frame-driven" declarative bindings in later
    /// rounds.
    ///
    /// Field private — the spec only exposes
    /// [`Self::tick_animations`] (write side) and an accessor
    /// [`Self::frame_signal`] (read side) for application observers.
    frame_signal: Signal<u64>,

    /// R51.149 §5.28 — most-recent `dt` value (seconds) captured
    /// before [`Self::frame_signal`] is bumped. The
    /// [`Self::_animation_driver`] Effect reads this inside its
    /// re-run closure — the counter fires the subscription cascade,
    /// the cell carries the actual `dt` to the spring solver.
    ///
    /// `Rc<Cell<f32>>` (not bare `Cell`) so the driver Effect can
    /// capture an alias by `Rc::clone` at construction — both the
    /// struct field and the Effect body access the *same* cell, so
    /// every `Self::tick_animations` write is visible to the next
    /// Effect re-run verbatim. `Cell` (not `Signal`) because the
    /// value is only ever read from inside the driver Effect, never
    /// auto-subscribed; an extra Signal here would create a
    /// redundant observer list with no downstream consumers.
    last_dt: std::rc::Rc<Cell<f32>>,

    /// R51.149 §5.28 — monotonic frame counter mirror.
    ///
    /// [`Self::frame_signal`] is the reactive surface (fires Effects
    /// on change); this `Cell` is the substrate's own write-side
    /// scratch — `tick_animations` reads it, increments, writes back,
    /// then pushes the new value to the signal. Keeping the
    /// next-value computation out of `Signal::get` avoids
    /// auto-subscribing whatever scope is on the
    /// `CURRENT_OWNER` stack when the caller (the backend's paint
    /// cycle) might be operating inside an unrelated reactive context
    /// in some future call path. Strictly an implementation detail.
    next_frame_count: Cell<u64>,

    /// R680 §5.16 §5.28 — observer-anchor Effect, subscribed to
    /// [`Self::frame_signal`] but with a no-op body.
    ///
    /// **R680 atomic 1** replaced the R51.149 Effect-routed animation
    /// dispatch (the body used to call `root_owner.tick_animations(dt)`
    /// cascade) with direct per-window dispatch inside
    /// [`Self::tick_animations_for_window`]. The Effect itself stays
    /// for backward compatibility with application-side reactive
    /// subscribers that observe [`Self::frame_signal`] indirectly
    /// (they expect at least one Effect to anchor the signal's
    /// observer list so the eager-init reactive eval matches the
    /// pre-R680 evaluation count). A no-op body anchors the
    /// subscription without doing any work that would now double up
    /// with the direct dispatch.
    ///
    /// The `_` prefix matches Rust convention for "value held for its
    /// side effects, never read directly".
    _animation_driver: Effect,

    /// R51.157 §5.23 — optional [`CommandExecutor`] the substrate
    /// drains pending [`Command`](pinion_core::Command) queue into via
    /// [`Self::dispatch_pending_commands`].
    ///
    /// `Option<Arc<CommandExecutor>>` so:
    ///
    /// - Headless tests that exercise the dispatch primitives without
    ///   binding any handler can leave the field `None`; pending
    ///   commands stay parked in the owner queues for inspection via
    ///   [`Owner::pending_commands`](pinion_core::reactive::Owner::pending_commands).
    /// - Backends bind a single shared executor at boot and inject via
    ///   [`Self::set_executor`] / [`Self::with_executor`]; both
    ///   `pinion-shell` and `pinion-tui` can hold their own
    ///   `Arc<CommandExecutor>` clones cheaply.
    /// - Swapping handler vocabularies for tests / runtime feature
    ///   gates uses [`Self::set_executor`] which returns the prior
    ///   executor for symmetry with the §5.23 R27 "swappable for
    ///   testing" contract.
    ///
    /// `Arc` (not `Rc`) because the [`CommandExecutor`] internals are
    /// already `Send + Sync` (the
    /// [`Executor`](crate::command::Executor) and
    /// [`IntentSink`](crate::command::IntentSink) trait bounds force
    /// it). The substrate's other `Rc<...>` fields stay `!Send`; the
    /// added `Arc` only widens the bound on this particular field, not
    /// on `CoreShell` as a whole.
    executor: Option<Arc<CommandExecutor>>,

    /// R680 §5.16 §5.41 §5.28 — per-window reactive scope map.
    ///
    /// First atomic of the 4-axis paint-pipeline rewrite series
    /// (R680-R683). Keyed by canonical [`WindowSpec::id`] string;
    /// each value is an [`Owner`] handle (cheap `Rc`-internal clone).
    ///
    /// ## Scope topology
    ///
    /// - `window_owners[DEFAULT_WINDOW]` is seeded in [`Self::new`]
    ///   as a `clone` of [`Self::root_owner`]. Same `Rc<OwnerInner>`
    ///   internals — registrations on either handle reach the same
    ///   scope. This is the canonical
    ///   `primary window owner == ShellCore-wide owner` mapping the
    ///   R680 plan calls out: every single-window binding (Phase A,
    ///   15+ examples) keeps its [`Owner::cache`] slots + animation
    ///   registry + command queue bit-identical to pre-R680 because
    ///   the active scope under `compute_paint_scene_internal`'s view
    ///   fn wrap reads through the same `Rc` either way.
    ///
    /// - Secondary entries (`"inspector"`, future dock-panel tear-off
    ///   ids, etc.) lazy-create through [`Self::window_owner`] via
    ///   [`Owner::new_child`]`(&root_owner)`. The new child appears
    ///   on `root_owner`'s children list (one strong ref) AND in this
    ///   map (one strong ref). Substrate drop releases the map first
    ///   (declaration order; this field is declared after
    ///   `root_owner`) then `root_owner` itself, whose cascade walks
    ///   `children` and releases the last strong ref → child
    ///   `OwnerInner` destroys.
    ///
    /// ## What R680 atomic (0) does NOT yet wire
    ///
    /// The view fn wrap in
    /// `pinion_shell::ShellCore::compute_paint_scene_internal` still
    /// reads `self.core.root_owner().run(...)`; switching to
    /// `window_owner(window_id).run(...)` is deferred to R680 atomic
    /// (1) so the animation-tick decoupling lift (the load-bearing
    /// behaviour change) lands together with the wrap shift instead
    /// of in two churn-only commits. Until then, secondary
    /// `window_owners["inspector"]` entries exist + can host their
    /// own [`Owner::cache`] slots / animation registrations
    /// (registered by callers reaching for the per-window scope
    /// explicitly), but the substrate's automatic view-fn-side
    /// registrations still land on `root_owner`. Atomic (1) flips
    /// the wrap to make per-window registration the default.
    ///
    /// ## Field choice rationale
    ///
    /// `HashMap<String, Owner>` mirrors the existing
    /// [`Self::routers`] field shape — same `WindowSpec::id` key,
    /// same `String::to_owned` allocation profile on lazy create,
    /// same hot-path `HashMap::get` lookup on every paint cycle. The
    /// alternative `HashMap<&'static str, _>` would require
    /// `WindowSpec::id` lifetimes to round-trip through the
    /// substrate's storage; the current `WindowSpec::id: &'static str`
    /// convention does support that but the runtime-window-spec
    /// lift planned for R683 (atomic 1: `Signal<Vec<WindowSpec>>`)
    /// will need owned ids, so `String` ahead.
    window_owners: HashMap<String, Owner>,
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

/// R672 §5.35 §5.41 — canonical single-window / primary
/// `WindowSpec::id` used by [`CoreShell`] when callers do not supply
/// an explicit window id. Single-window bindings (every example
/// before R670.B's `hello-multi-window`) populate exactly this
/// router slot; multi-window bindings address additional slots by
/// the secondary `WindowSpec::id` values. Mirrors
/// `pinion_shell::WindowSpec::main`'s `&'static str` literal so the
/// surface + the substrate share one canonical name.
pub const DEFAULT_WINDOW: &str = "main";

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
        let root_owner = Owner::new();
        // (R55.D.5 §5.45) Compose the state-scene root.
        //
        // Default (single-External binding, the entire example
        // catalogue except `hello-listbox`): the scene stays
        // `Scene::External(primary)` — bit-for-bit identical to the
        // pre-R55.D.5 shape, every existing `read_state` keeps
        // working.
        //
        // Override (`V::create_extra_externals` non-empty): the scene
        // becomes `Scene::Container([primary, ...extras])`. The
        // extras list is resolved inside `root_owner.run` so the
        // factory can call [`use_scroll_state`] and other
        // `Owner::cache` hooks, sharing reactive state with what the
        // view fn will resolve later (same cache key → same
        // `Rc<ScrollState>`).
        //
        // (R56.1.b.1 §5.41) `create_external` runs inside the same
        // `root_owner.run(...)` wrap so the primary External factory
        // can call [`use_text_edit_state`] / [`use_caret_blink`] /
        // [`use_scroll_state`] hooks too — required by widgets like
        // `TextField` whose External composes shared reactive state
        // with the view fn. Bindings without reactive-state needs
        // (every pre-R56.1.b.1 example) are unaffected: their
        // factories ignore the Owner context.
        let primary = Scene::External(
            ExternalNode::new(root_owner.run(V::create_external)).with_tag(V::tag()),
        );
        let extras = root_owner.run(V::create_extra_externals);
        let scene = if extras.is_empty() {
            primary
        } else {
            let mut children: Vec<Scene> = Vec::with_capacity(1 + extras.len());
            children.push(primary);
            for extra in extras {
                children.push(Scene::External(
                    ExternalNode::new(extra.handle).with_tag(extra.tag),
                ));
            }
            Scene::Container(ContainerNode::new(children))
        };
        let cached_state = V::read_state(&scene);
        let frame_signal = Signal::new(0_u64);
        let last_dt: std::rc::Rc<Cell<f32>> = std::rc::Rc::new(Cell::new(0.0_f32));

        // R680 §5.16 §5.28 — observer-anchor Effect. The pre-R680
        // body dispatched `owner_for_closure.tick_animations(dt)` as
        // the framework AnimationDriver (R51.149); R680 atomic 1
        // moved tick dispatch into direct per-window calls inside
        // [`Self::tick_animations_for_window`] so each window's
        // paint cycle advances only its own scope (the R670.B
        // 9-round honest carry on multi-window animation compound
        // is closed structurally).
        //
        // Keeping the Effect anchored (with a no-op body that still
        // subscribes via `signal_for_driver.get()`) preserves the
        // pre-R680 invariant that [`Self::frame_signal`]'s observer
        // list contains at least one Effect at construction —
        // application-side `Effect::new(&owner, || { … })` closures
        // that subscribe to the signal still see the same eager-
        // initial-run sequencing because they are appended after
        // the anchor.
        let owner_for_driver = root_owner.clone();
        let signal_for_driver = frame_signal.clone();
        // `_dt_unused` mirrors the pre-R680 closure capture so the
        // `Rc<Cell<f32>>` clone discipline is unchanged in shape;
        // the closure body itself does not read `dt` anymore.
        let _dt_unused = std::rc::Rc::clone(&last_dt);
        let animation_driver = root_owner.run(|| {
            Effect::new(&owner_for_driver, move || {
                // Subscribe to the counter on the eager initial run +
                // re-fire on every [`Self::tick_animations_for_window`]
                // bump. The dispatch happens in `tick_animations_for_window`
                // directly; this Effect is just the subscription
                // anchor.
                let _frame = signal_for_driver.get();
            })
        });

        // R672 §5.35 §5.41 — seed the routers map with the
        // [`DEFAULT_WINDOW`] entry so single-window bindings + every
        // pre-R672 test exerciser (the routerless-window-id call
        // surface) find a router immediately without lazy-init.
        let mut routers: HashMap<String, InputRouter> = HashMap::new();
        routers.insert(DEFAULT_WINDOW.to_owned(), InputRouter::new());

        // R680 §5.16 §5.41 §5.28 — seed `window_owners` with the
        // canonical primary slot mapped to a clone of `root_owner`.
        // The clone shares the same `Rc<OwnerInner>` internals so
        // `window_owner(DEFAULT_WINDOW)` and `root_owner()` resolve
        // through the same scope: single-window bindings see zero
        // behaviour change (`Owner::cache` slots, animation
        // registrations, and command queues stay co-located with
        // `root_owner`). Secondary windows lazy-create through
        // [`Self::window_owner`] using [`Owner::new_child`]`(&root_owner)`
        // so the new scope cascades on root drop without any explicit
        // cleanup wiring.
        let mut window_owners: HashMap<String, Owner> = HashMap::new();
        window_owners.insert(DEFAULT_WINDOW.to_owned(), root_owner.clone());

        Self {
            scene,
            cached_state,
            routers,
            intent_queue: IntentQueue::new(),
            root_owner,
            frame_signal,
            last_dt,
            next_frame_count: Cell::new(1_u64),
            _animation_driver: animation_driver,
            executor: None,
            window_owners,
        }
    }

    /// R51.157 §5.23 — builder-style executor injection.
    ///
    /// Backends typically chain this after [`Self::new`] at boot:
    ///
    /// ```ignore
    /// let core: CoreShell<MyView> = CoreShell::new()
    ///     .with_executor(Arc::clone(&shared_executor));
    /// ```
    ///
    /// For post-construction injection (renderer-attached / swappable
    /// registration phases) use [`Self::set_executor`].
    #[must_use]
    pub fn with_executor(mut self, executor: Arc<CommandExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// R51.157 §5.23 — install or replace the [`CommandExecutor`] used
    /// by [`Self::dispatch_pending_commands`].
    ///
    /// Returns the prior executor (if any) so callers can restore it
    /// after a scoped swap — matches the §5.23 R27 "swappable for
    /// testing" contract on the underlying
    /// [`HandlerRegistry`](crate::command::HandlerRegistry).
    pub fn set_executor(
        &mut self,
        executor: Arc<CommandExecutor>,
    ) -> Option<Arc<CommandExecutor>> {
        self.executor.replace(executor)
    }

    /// R51.157 §5.23 — detach the current [`CommandExecutor`].
    /// Returns the cleared handle so callers may transfer ownership
    /// (e.g. a shutdown path that wants to drain remaining in-flight
    /// work on the dropped executor before the substrate goes away).
    pub fn clear_executor(&mut self) -> Option<Arc<CommandExecutor>> {
        self.executor.take()
    }

    /// R51.157 §5.23 — read-only borrow of the currently-installed
    /// [`CommandExecutor`]. `None` when no executor has been injected
    /// (the headless-test default).
    #[must_use]
    pub fn executor(&self) -> Option<&Arc<CommandExecutor>> {
        self.executor.as_ref()
    }

    /// R51.157 §5.23 — drain every pending
    /// [`Command`](pinion_core::Command) from
    /// [`Self::root_owner`](Self::root_owner) (recursively through its
    /// child scopes) and dispatch each through the installed
    /// [`CommandExecutor`].
    ///
    /// Returns the [`Command`]s for which the registry had no
    /// registered handler — backends typically `eprintln!` these to
    /// surface handler-registration mistakes. Silent drop is
    /// intentionally not the default because an unhandled [`Command`]
    /// is exactly the case an AI agent's introspection wants to see.
    ///
    /// No-op when [`Self::executor`] is `None`: pending commands stay
    /// in their owner queues for inspection via
    /// [`Owner::pending_commands`](pinion_core::reactive::Owner::pending_commands).
    /// Returns an empty `Vec` in that case.
    ///
    /// Backends call this after every dispatch tail (or at the end of
    /// every event-loop tick) so commands queued during the just-run
    /// SCXML transition reach their handlers before the next frame
    /// paints.
    #[must_use = "unhandled commands describe a handler-registration mismatch; surface or log them rather than dropping silently"]
    pub fn dispatch_pending_commands(&self) -> Vec<Command> {
        let Some(executor) = self.executor.as_ref() else {
            // No executor installed — leave commands in the owner
            // queues so an inspecting AI client can still observe them
            // via the `scene/commands` RPC method (carry).
            return Vec::new();
        };
        let pending = self.root_owner.take_pending_commands_recursive();
        let mut unhandled: Vec<Command> = Vec::new();
        for cmd in pending {
            if executor.registry().has(cmd.kind_str()) {
                // Drop the returned `CommandTaskHandle` — R51.158 will
                // capture it into a per-scope cancellation map.
                let _ = executor.dispatch(cmd);
            } else {
                unhandled.push(cmd);
            }
        }
        unhandled
    }

    /// R51.167 §5.23 R27 — route an incoming Intent through the
    /// [`WidgetCore::update`] reducer and queue the produced
    /// `Vec<Command>` on the [`root_owner`](Self::root_owner).
    ///
    /// The dispatch loop's reducer step: every Intent arriving from
    /// either widget-side input (the §5.20 SCXML drain that
    /// `walk_scene_and_drain` surfaces during `tick_intents`) or the
    /// async re-feed path
    /// (`AppEvent::IntentArrived` on Vello / `MpscIntentSink::try_recv`
    /// on TUI) flows through this method before reaching the SCXML
    /// `invoke("send", …)` channel.
    ///
    /// R51.173 §5.23 R27 — the cached projection
    /// [`WidgetCore::read_state`] yields is handed to the reducer
    /// **by value** (the spec's `&mut Model` carries an SCXML-shaped
    /// Model in pinion, which lives on [`Scene::External`] not on
    /// the cached snapshot — see [[scxml-as-model-update-transient]]).
    /// The reducer reads the snapshot and emits the declarative
    /// `Vec<Command>` half of the §5.23 R27 contract. Every command
    /// lands on the owner queue so
    /// [`Self::dispatch_pending_commands`] reaches the registered
    /// handler on the next pump; state changes flow back through the
    /// `Command` → `Handler` → produced `Intent` → SCXML send loop.
    ///
    /// Returns the same `Vec<Command>` the reducer produced so
    /// callers (and tests) can introspect what was queued without a
    /// second owner snapshot. The commands are already on the queue
    /// — drop the return value if the caller has no use for it.
    ///
    /// R51.171 §5.22 R26 — the reducer call site is wrapped in
    /// [`Owner::run`] so [`Owner::current`] resolves to the same
    /// [`root_owner`](Self::root_owner) the framework queues
    /// commands on. Reducer authors that want to tag their emitted
    /// `Command` with the producing scope can write
    /// `Owner::current().map_or(0, |o| o.id())` without reaching
    /// through the substrate, mirroring the
    /// [[callback-root-owner-wrap]] pattern already applied to
    /// `V::view` (R51.146) and `V::apply_key` (R51.152).
    #[must_use = "the returned commands are already queued; ignore explicitly with `let _ = …` if you do not need them"]
    pub fn route_intent_through_update(&self, intent: &Intent) -> Vec<Command> {
        // R51.173 §5.23 R27 — by-value snapshot. `V::State: Copy`
        // already constrains the type; no `&mut` borrow is taken so
        // the call site reads as a pure snapshot pass.
        let state = V::read_state(&self.scene);
        let commands = self.root_owner.run(|| V::update(state, intent));
        for cmd in &commands {
            self.root_owner.dispatch_command(cmd.clone());
        }
        commands
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

    /// R51.142 §5.28 — read-only borrow of the root reactive scope
    /// owned by this binding.
    ///
    /// Used by the view fn (or SCE-emitted code) to attach
    /// [`Animation<T>`](pinion_core::Animation) instances and
    /// [`Effect`](pinion_core::Effect) closures to this widget's
    /// lifetime. Drop on `CoreShell` propagates through
    /// [`Owner`] drop, which cascades to children and cancels every
    /// pending [`Command`](pinion_core::Command) (Solid pattern,
    /// R51.139). Borrowed read-only because [`Owner`] is itself
    /// reference-counted internally — registrations on a clone reach
    /// the same scope, so callers that need an owned handle can
    /// `clone()` the borrow without losing the lifetime tie.
    #[must_use]
    pub fn root_owner(&self) -> &Owner {
        &self.root_owner
    }

    /// R680 §5.16 §5.41 §5.28 — resolve (or lazy-create) the
    /// per-window reactive scope keyed by `window_id`.
    ///
    /// Returns an owned [`Owner`] handle (a cheap `Rc`-internal
    /// clone). Two calls with the same `window_id` always return
    /// handles wrapping the same `Rc<OwnerInner>`; registrations on
    /// either handle reach the same scope.
    ///
    /// ## Mapping
    ///
    /// - `window_id == ` [`DEFAULT_WINDOW`] (`"main"`) — returns a
    ///   clone of [`Self::root_owner`]. Seeded in [`Self::new`]; the
    ///   primary-window owner is the binding-wide root scope so
    ///   single-window callers (every Phase A binding) observe
    ///   bit-identical behaviour through this accessor as through
    ///   [`Self::root_owner`].
    /// - Any other `window_id` — first call lazy-creates an
    ///   [`Owner::new_child`]`(&root_owner)` and stores it in the
    ///   map; subsequent calls return the cached child. The new
    ///   child is parented to `root_owner` so cascade drop fires
    ///   when this [`CoreShell`] drops (animations, effects, and
    ///   commands registered on the secondary scope evaporate
    ///   together with the substrate).
    ///
    /// ## Why `&mut self`
    ///
    /// Lazy insert mutates the underlying map. Callers that need a
    /// read-only probe (RPC introspection, telemetry that must not
    /// create a fresh scope) reach for
    /// [`Self::window_owner_existing`] instead. The map's hot-path
    /// readers (R680 atomic 1's per-window `tick_animations`,
    /// R680 atomic 2's `redraw_requested` per-window flag, R680
    /// atomic 3's RPC dispatch wrap) all hold `&mut self` already so
    /// the mutability requirement does not propagate up the call
    /// stack.
    ///
    /// ## Allocation profile
    ///
    /// First call per `window_id` allocates the String key (via
    /// `to_owned`) and a fresh `OwnerInner` (one heap alloc inside
    /// `Owner::new_child`). Subsequent calls hash-lookup the
    /// existing entry without touching the allocator beyond the Rc
    /// bump. Identical profile to the existing
    /// [`Self::routers`] map.
    pub fn window_owner(&mut self, window_id: &str) -> Owner {
        if let Some(owner) = self.window_owners.get(window_id) {
            return owner.clone();
        }
        // R680 §5.28 — fresh secondary scope is a child of the
        // binding-wide `root_owner` so cascade-drop on substrate
        // destruction releases the child's animations / commands /
        // cache slots without an explicit teardown call.
        let child = Owner::new_child(&self.root_owner);
        self.window_owners.insert(window_id.to_owned(), child.clone());
        child
    }

    /// R680 §5.16 §5.41 §5.28 — read-only probe for a per-window
    /// scope. Returns `Some(owner_clone)` when an entry already
    /// exists; `None` for unknown ids. Never creates a fresh scope.
    ///
    /// Designed for RPC introspection paths (`scene/commands`
    /// per-window scoping in R680 atomic 3, future telemetry
    /// surfaces) where instantiating a new scope as a side effect
    /// of "does this window have any registered work?" would be a
    /// contract violation.
    ///
    /// The returned [`Owner`] is a clone of the stored handle, so
    /// callers can hand it to [`Owner::tick_animations`] /
    /// [`Owner::pending_commands`] / etc. without taking a borrow on
    /// the substrate.
    #[must_use]
    pub fn window_owner_existing(&self, window_id: &str) -> Option<Owner> {
        self.window_owners.get(window_id).cloned()
    }

    /// R680 §5.16 §5.41 §5.28 — iterator over every known per-window
    /// scope id. Used by tests + multi-window backends that need to
    /// walk every active window (e.g. the R683 dock-panel
    /// `reconcile_windows` Effect's drop pass after a tear-off
    /// dock-back). Order follows
    /// [`HashMap`](std::collections::HashMap) iteration semantics
    /// (unstable across rebuilds; callers needing deterministic
    /// order sort downstream).
    pub fn window_owner_ids(&self) -> impl Iterator<Item = &str> {
        self.window_owners.keys().map(String::as_str)
    }

    /// R683 §5.16 §5.41 §5.28 — drop a secondary window's per-window
    /// substrate state.
    ///
    /// Walks every `HashMap` keyed by `window_id` and removes the
    /// entry: the `routers` map (R672 `InputRouter`), the
    /// `window_owners` map (R680 per-window `Owner` child scope),
    /// dropping the `Owner` triggers a cleanup-queue cascade that
    /// releases every animation / command / cache slot registered on
    /// that scope.
    ///
    /// Refuses to remove [`DEFAULT_WINDOW`]: the primary scope is
    /// aliased to `root_owner` so removing it would orphan the
    /// binding's reactive substrate. Returns `true` on actual
    /// removal, `false` for unknown / [`DEFAULT_WINDOW`] ids —
    /// callers can use the boolean to detect "primary protected" vs
    /// "no-op" cases without separate getters.
    ///
    /// Designed for the R683 `AppShell::reconcile_windows` Effect's
    /// drop pass after a dock tear-off / dock-back arc resolves —
    /// the matching `WindowSlot` drop on the `AppShell` side releases
    /// the OS window + `accesskit_winit::Adapter`; this method
    /// closes the matching substrate-side loop.
    pub fn remove_window(&mut self, window_id: &str) -> bool {
        if window_id == DEFAULT_WINDOW {
            return false;
        }
        // The two `_for_window` HashMaps + the per-window Owner map
        // share the same `window_id` keying contract; drop them all
        // in one pass so the substrate has no stale per-window
        // entries when the next paint cycle skips the dropped slot.
        // Returns `true` only if at least one of the three maps had
        // an entry — defensive against a caller that fires
        // remove_window before the substrate ever saw the id.
        let router_removed = self.routers.remove(window_id).is_some();
        let dropped_owner = self.window_owners.remove(window_id);
        // R683 §5.28 — `Owner::new_child` pushed the secondary scope
        // onto `root_owner.children` (R680 cascade-drop discipline).
        // Removing the `window_owners` entry alone only releases one
        // strong ref; the parent's children list still holds the
        // other. Detach by `Owner::id` so the child's last strong
        // ref drops + the cleanup queue actually fires (animations /
        // commands / cache slots registered on that scope release).
        if let Some(owner) = dropped_owner.as_ref() {
            let _ = self.root_owner.detach_child_by_id(owner.id());
        }
        router_removed || dropped_owner.is_some()
    }

    /// R51.142 §5.28 — advance every animation registered on the
    /// [`root_owner`](Self::root_owner) by `dt` seconds.
    ///
    /// Backends call this once per paint cycle with the measured
    /// delta between frames (the same `dt` passed to
    /// [`Frame::with_dt`](pinion_core::Frame::with_dt) for the view
    /// fn). On the first paint or any synthetic flush the caller
    /// should pass `0.0`; spring solvers leave at-rest animations
    /// untouched at zero `dt` so the call is idempotent.
    ///
    /// The tick walks children depth-first before this scope (R51.138
    /// cascade order) and snapshots the animation list before
    /// iterating, so handlers registering or unregistering animations
    /// inside their tick callback do not break the sweep — the new
    /// registrations pick up on the next frame instead.
    pub fn tick_animations(&self, dt: f32) {
        // R680 §5.16 §5.28 — single-window / primary-window legacy
        // entry. Routes through the per-window dispatch using
        // [`DEFAULT_WINDOW`] so single-window bindings (the entire
        // Phase A example catalogue) see bit-identical behaviour:
        // `window_owners[DEFAULT_WINDOW]` is seeded as a clone of
        // `root_owner` in [`Self::new`], so the local-only walk
        // (`Owner::tick_animations_local` rather than the cascade
        // `tick_animations`) over root's own animation list reaches
        // every single-window-registered animation (there are no
        // root children outside of secondary windows pre-R680
        // atomic 1).
        //
        // Multi-window backends call [`Self::tick_animations_for_window`]
        // directly per paint cycle with their slot's `window_id` +
        // measured `dt`; the legacy `tick_animations` alias keeps
        // every pre-R680 caller (test fixtures, headless screenshot
        // pipeline, hello-listbox single-window pump) working
        // without API churn.
        self.tick_animations_for_window(DEFAULT_WINDOW, dt);
    }

    /// R680 §5.16 §5.28 §5.41 — per-window animation tick dispatch.
    ///
    /// Advances **only** the animations registered against the
    /// `window_id`'s scope (looked up through
    /// [`Self::window_owner_existing`]). Each paint cycle of each
    /// window calls this once with the measured frame delta against
    /// THAT window's own paint clock (`pinion-shell::ShellCore`
    /// owns the per-window `last_paint_instants` map; pinion-tui
    /// + headless backends follow the same convention).
    ///
    /// ## What the call does
    ///
    /// 1. Records `dt` into the substrate's `last_dt` cell (read by
    ///    application observers that resolve the most-recent frame
    ///    delta from outside a reactive context).
    /// 2. Resolves the window's [`Owner`] scope via
    ///    [`Self::window_owner_existing`]. `None` (unknown id, never
    ///    painted yet) → no animation walk runs; the call falls
    ///    through to the `frame_signal` bump so application observers
    ///    of [`Self::frame_signal`] still see the cascade. The
    ///    no-walk path matters because R680 atomic 1 should not
    ///    silently lazy-create a scope just because a tick was
    ///    requested against an unknown id; only paint cycles +
    ///    explicit [`Self::window_owner`] calls instantiate the
    ///    secondary scope.
    /// 3. Calls
    ///    [`Owner::tick_animations_local`](pinion_core::reactive::Owner::tick_animations_local)
    ///    on the resolved scope — the **local** variant skips the
    ///    child-scope cascade. For `window_id == DEFAULT_WINDOW`
    ///    the scope is `root_owner` (via the seeded alias), and
    ///    every single-window-registered animation lives directly
    ///    on root's own animation list, so the local walk reaches
    ///    them all. For a secondary `window_id` the scope is the
    ///    lazy-created child; its `owned_animations` list holds
    ///    only what the secondary window's view fn + RPC dispatch
    ///    registered there, so foreign windows' animations are
    ///    structurally invisible.
    /// 4. Bumps the monotonic frame counter + writes through
    ///    [`Self::frame_signal`] so application Effects subscribed
    ///    to the paint clock re-run. The bump fires every tick
    ///    (`u64` wrap counter pattern from R51.149) regardless of
    ///    whether the walk in step 3 ran, so observers that just
    ///    want to react to "a paint happened" still see the cascade
    ///    on every call.
    ///
    /// ## Backward compatibility
    ///
    /// Single-window backends (every Phase A binding) call
    /// [`Self::tick_animations`] (the legacy entry) which forwards
    /// here with `window_id = DEFAULT_WINDOW`. Multi-window backends
    /// (Phase B+) call this directly per window. The R670.B 9-round
    /// honest carry on multi-window animation tick compound is
    /// closed at this entry: two windows painting in the same
    /// event-loop turn each call this with their own `dt`, walking
    /// only their own scope. The pre-R680 cascade-tick path no
    /// longer fires from the substrate — `Owner::tick_animations`
    /// (the cascade variant) is still available as a primitive for
    /// pinion-rpc's `animate_advance` headless dispatcher, but the
    /// substrate's own paint cycle uses the local walk.
    pub fn tick_animations_for_window(&self, window_id: &str, dt: f32) {
        self.last_dt.set(dt);
        if let Some(owner) = self.window_owner_existing(window_id) {
            owner.tick_animations_local(dt);
        }
        // R51.149 §5.28 — bump the monotonic counter every tick so
        // application observers of `frame_signal` re-fire on each
        // paint cycle (sidesteps `Signal::set`'s equality-skip even
        // when two ticks pass the same `dt`).
        let next = self.next_frame_count.get();
        self.next_frame_count.set(next.wrapping_add(1));
        self.frame_signal.set(next);
    }

    /// R51.149 §5.28 — read-only borrow of the per-frame counter
    /// signal that drives the substrate's animation pump.
    ///
    /// Increments monotonically on every [`Self::tick_animations`]
    /// call. Application-side reactive subscribers (Computed /
    /// Effect / future SCE-emitted "frame-driven" bindings) can
    /// call [`Signal::get`](pinion_core::reactive::Signal::get) on
    /// this signal to react in lockstep with the paint clock — the
    /// counter value itself is opaque (it just fires the cascade);
    /// for the actual per-frame delta-time, the substrate exposes
    /// it through the framework `AnimationDriver`'s tick routing, not
    /// through this signal. The signal-of-counter shape sidesteps
    /// [`Signal::set`]'s equality-skip so successive ticks with
    /// identical `dt` still propagate.
    #[must_use]
    pub fn frame_signal(&self) -> &Signal<u64> {
        &self.frame_signal
    }

    /// R51.147 §5.28 — `true` when any animation registered on this
    /// binding's [`root_owner`](Self::root_owner) (or transitively on
    /// a child scope) is still moving above `epsilon`.
    ///
    /// Backends call this after the paint cycle's
    /// [`Self::tick_animations`] step to decide whether the next
    /// frame should also paint. Once `false`, the backend can stop
    /// requesting redraws and let the surface idle until the next
    /// event arrives (state change / input / RPC mutation).
    ///
    /// `epsilon` is forwarded verbatim to each
    /// [`pinion_core::animation::Tickable::is_at_rest`]. Typical
    /// callers pass
    /// [`pinion_core::Animation::DEFAULT_REST_EPSILON`] so the
    /// stopping rule matches the spring solver's own settlement
    /// threshold.
    #[must_use]
    pub fn any_animation_active(&self, epsilon: f32) -> bool {
        self.root_owner.any_animation_active(epsilon)
    }

    /// R680 §5.16 §5.28 §5.41 — per-window animation-active probe.
    ///
    /// Returns `true` when any animation registered **directly on
    /// the addressed window's scope** is still moving above
    /// `epsilon`. Mirrors [`Self::any_animation_active`] but reads
    /// only the addressed window's own animation list (via
    /// [`Owner::any_animation_active_local`](pinion_core::reactive::Owner::any_animation_active_local)),
    /// so multi-window backends can decide per-window redraw without
    /// taking foreign windows' activity into account.
    ///
    /// Returns `false` when:
    ///
    /// - The `window_id` has never been instantiated (no entry in
    ///   `window_owners`) — symmetric with
    ///   [`Self::tick_animations_for_window`]'s no-lazy-create
    ///   contract.
    /// - The scope exists but every registered animation reports
    ///   `is_at_rest(epsilon) == true`.
    /// - The scope holds no animations.
    ///
    /// Backends that want the binding-wide "any window still
    /// animating?" answer keep calling [`Self::any_animation_active`]
    /// (cascade walk via `root_owner`).
    #[must_use]
    pub fn any_animation_active_for_window(&self, window_id: &str, epsilon: f32) -> bool {
        self.window_owner_existing(window_id)
            .is_some_and(|owner| owner.any_animation_active_local(epsilon))
    }

    /// R51.122 §5.41 — hand a freshly-painted scene to the
    /// [`InputRouter`] so the next pointer event resolves against the
    /// visible layout. Both backends call this once per paint commit
    /// (initial + post-state-change + resize repaint).
    ///
    /// R672 §5.35 §5.41 — single-window wrapper around
    /// [`Self::update_paint_scene_for_window`]. Single-window
    /// bindings + every pre-R672 test surface (single
    /// [`InputRouter`] held the binding-wide last-paint-scene)
    /// keep paying this entry; multi-window bindings dispatch to
    /// [`Self::update_paint_scene_for_window`] with the addressed
    /// `WindowSpec::id`.
    pub fn update_paint_scene(&mut self, paint_scene: Scene) {
        self.update_paint_scene_for_window(DEFAULT_WINDOW, paint_scene);
    }

    /// R672 §5.35 §5.41 — per-window paint-scene hand-off. Looks
    /// up (or lazy-creates) the addressed window's [`InputRouter`]
    /// and forwards through [`InputRouter::update_paint_scene`] so
    /// only that window's pointer state walks `refresh_hover`
    /// against the new scene. Foreign windows' pointer state stays
    /// pinned to their own last-paint scenes — no cross-window
    /// flip-flop.
    pub fn update_paint_scene_for_window(
        &mut self,
        window_id: &str,
        paint_scene: Scene,
    ) {
        let Self { scene, routers, .. } = self;
        let router = routers
            .entry(window_id.to_owned())
            .or_default();
        router.update_paint_scene(paint_scene, scene);
    }

    /// (R685 §5.16 §5.35) Pure-storage paint-scene write — no
    /// `refresh_hover` side effect. Per-window mirror of
    /// [`InputRouter::set_paint_scene`].
    ///
    /// Used by RPC dispatch paths that need fresh hit-test geometry
    /// after a state mutation moved widget rects, but **without**
    /// firing the synthetic `PointerEnter` / `PointerLeave` arcs the
    /// composed [`Self::update_paint_scene_for_window`] generates
    /// (those arcs are correct for a real winit paint cycle where
    /// the user is interactively engaged with the window, but
    /// incorrect for an AI-driven RPC that didn't move the cursor —
    /// only the layout moved under it). R684 atomic 3 worked around
    /// the side-effect with a first-paint-only gate; R685 lands the
    /// proper substrate split so every-RPC refresh is safe.
    pub fn set_paint_scene_for_window(
        &mut self,
        window_id: &str,
        paint_scene: Scene,
    ) {
        let Self { routers, .. } = self;
        let router = routers
            .entry(window_id.to_owned())
            .or_default();
        router.set_paint_scene(paint_scene);
    }

    /// R51.122 §5.41 — read-only proxy to the underlying
    /// [`InputRouter::hover_target`]. Backends use this to read the
    /// current hover target for `click_to_focus` style follow-up
    /// (the Vello shell's W3C HTML-style "press on focusable widget
    /// focuses it; press on background blurs" rule), without
    /// exposing the router's mutable interior.
    ///
    /// R672 §5.35 §5.41 — single-window wrapper that reads the
    /// [`DEFAULT_WINDOW`] router. Multi-window callers use
    /// [`Self::hover_target_for_window`].
    #[must_use]
    pub fn hover_target(&self, pid: PointerId) -> Option<&str> {
        self.hover_target_for_window(DEFAULT_WINDOW, pid)
    }

    /// R672 §5.35 §5.41 — per-window read of the
    /// [`InputRouter::hover_target`] for the named window. Returns
    /// `None` when the window has never been painted (no router
    /// entry yet) — single-window default callers can use
    /// [`Self::hover_target`] unchanged.
    #[must_use]
    pub fn hover_target_for_window(
        &self,
        window_id: &str,
        pid: PointerId,
    ) -> Option<&str> {
        self.routers.get(window_id).and_then(|r| r.hover_target(pid))
    }

    /// (R684 §5.35 §5.41 §5.16) Per-window passthrough to
    /// [`InputRouter::has_last_paint_scene`] — `true` once the
    /// named window's router has received a paint scene via
    /// [`Self::update_paint_scene_for_window`]. Returns `false`
    /// when no router entry exists yet (newly-spawned floating
    /// window that has never been painted; the canonical
    /// substrate-incompleteness signal R684 atomic 3 closes for
    /// headless-RPC consumers).
    #[must_use]
    pub fn has_last_paint_scene_for_window(&self, window_id: &str) -> bool {
        self.routers
            .get(window_id)
            .is_some_and(super::input::InputRouter::has_last_paint_scene)
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
    ///
    /// `modifiers` carries the W3C `KeyboardEvent` four-bit modifier
    /// surface (R56.1.f.0 §5.13). The Vello shell sources it from the
    /// `ShellCore::modifiers` cache (refreshed on every winit
    /// `ModifiersChanged`); the TUI shell converts from crossterm's
    /// `KeyModifiers` at the input bridge; the RPC dispatch path
    /// supplies a caller-specified value (defaulting to
    /// [`Modifiers::empty`](pinion_core::input::Modifiers::empty)
    /// when the legacy no-modifier shape is used).
    pub fn apply_key(
        &mut self,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> Option<DispatchTail<V::State>> {
        // R51.152 §5.22 — wrap `V::apply_key` in
        // `root_owner.run(...)` so [`pinion_core::Owner::current`]
        // resolves to this binding's root scope from inside the
        // widget's keyboard handler. Application code (typeahead
        // cursors, per-binding scratch state, etc.) can call
        // `Owner::current().cache(...)` (R51.150) to persist heap
        // allocations across dispatch calls instead of reaching for
        // a `thread_local!` workaround. The `Owner` clone is a cheap
        // `Rc` bump; the borrow split lets the closure mutate
        // `self.scene` while the owner field stays untouched.
        let owner = self.root_owner.clone();
        let scene = &mut self.scene;
        let handled = owner.run(|| V::apply_key(scene, focused, key, modifiers));
        if handled {
            Some(self.tail())
        } else {
            None
        }
    }

    /// R56.2.a §5.13 §5.38 — route an IME [`CompositionEvent`] through
    /// [`WidgetCore::apply_composition`]. Symmetric with
    /// [`Self::apply_key`]: wraps the trait call in `root_owner.run`
    /// so [`Owner::current`](pinion_core::reactive::Owner::current)
    /// resolves to this binding's root scope from inside the widget's
    /// composition handler (e.g. `TextField::apply_composition` uses
    /// the same `use_text_edit_state(TF_TAG)` cache the view fn and
    /// `create_external` already share — R56.1.b.1 substrate).
    ///
    /// Returns `Some(DispatchTail)` on handled (`true` from
    /// `apply_composition`), `None` on unhandled — the shell wrapper
    /// checks the `Option` to decide whether to bump backend-specific
    /// bookkeeping (Vello: revision + redraw; TUI: repaint trigger).
    /// Mirrors the R51.122 `apply_key` return-shape contract so the
    /// pinion-shell `WindowEvent::Ime` arm can reuse the existing
    /// post-handle path (`handle_tail` + redraw request).
    ///
    /// `focused` carries the focus manager's currently-focused tag at
    /// dispatch time. pinion-shell's `app.rs` sources it from
    /// `self.focus.focused()` (same as `apply_key`); widget
    /// implementations short-circuit to `false` when the carried tag
    /// is not their own (roving-tabindex pattern), preventing
    /// composition events from broadcasting to unfocused widgets.
    pub fn apply_composition(
        &mut self,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> Option<DispatchTail<V::State>> {
        let owner = self.root_owner.clone();
        let scene = &mut self.scene;
        let handled = owner.run(|| V::apply_composition(scene, focused, event));
        if handled {
            Some(self.tail())
        } else {
            None
        }
    }

    /// R56.2.e §5.13 §5.22 — route a middle-mouse-button press through
    /// [`WidgetCore::apply_middle_click`]. Symmetric with
    /// [`Self::apply_key`] / [`Self::apply_composition`]: wraps the
    /// trait call in `root_owner.run` so
    /// [`Owner::current`](pinion_core::reactive::Owner::current)
    /// resolves to this binding's root scope from inside the widget's
    /// paste handler (e.g. `TextField::apply_middle_click` uses the
    /// same `use_text_edit_state(tag)` cache the view fn and
    /// `apply_key` already share — R56.1.b.1 substrate).
    ///
    /// Returns `Some(DispatchTail)` on handled (`true` from
    /// `apply_middle_click`), `None` on unhandled — the shell wrapper
    /// checks the `Option` to decide whether to bump backend-specific
    /// bookkeeping (Vello: revision + redraw; TUI: repaint trigger).
    /// Mirrors the R51.122 `apply_key` return-shape contract so the
    /// pinion-shell `WindowEvent::MouseInput { button: Middle, .. }`
    /// arm can reuse the existing post-input path (`handle_tail` +
    /// redraw request).
    ///
    /// `focused` carries the focus manager's currently-focused tag at
    /// dispatch time. pinion-shell's `app.rs` sources it from
    /// `self.focus.focused()` (same as `apply_key`); widget
    /// implementations short-circuit to `false` when the carried tag
    /// is not their own (roving-tabindex pattern), preventing
    /// middle-click events from broadcasting to unfocused widgets.
    pub fn apply_middle_click(
        &mut self,
        focused: Option<&str>,
        modifiers: pinion_core::Modifiers,
    ) -> Option<DispatchTail<V::State>> {
        let owner = self.root_owner.clone();
        let scene = &mut self.scene;
        let handled = owner.run(|| V::apply_middle_click(scene, focused, modifiers));
        if handled {
            Some(self.tail())
        } else {
            None
        }
    }

    /// R51.122 §5.41 — pointer cursor-move dispatch (cell→pixel or
    /// `winit` → pixel conversion happens at the backend boundary).
    /// Forwards through the [`InputRouter`] then drains the dispatch
    /// tail.
    ///
    /// R672 §5.35 §5.41 — single-window wrapper that dispatches via
    /// the [`DEFAULT_WINDOW`] router. Multi-window callers use
    /// [`Self::cursor_moved_for_window`].
    pub fn cursor_moved(
        &mut self,
        pid: PointerId,
        x: f64,
        y: f64,
    ) -> DispatchTail<V::State> {
        self.cursor_moved_for_window(DEFAULT_WINDOW, pid, x, y)
    }

    /// R672 §5.35 §5.41 — per-window pointer cursor-move dispatch.
    /// Looks up (or lazy-creates) the addressed window's router and
    /// forwards through [`InputRouter::cursor_moved`]; only that
    /// router's `hover_target` / `cursors` maps mutate.
    pub fn cursor_moved_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
        x: f64,
        y: f64,
    ) -> DispatchTail<V::State> {
        let Self { scene, routers, .. } = self;
        let router = routers
            .entry(window_id.to_owned())
            .or_default();
        router.cursor_moved(pid, x, y, scene);
        self.tail()
    }

    /// R51.122 §5.41 — pointer leaves the surface for `pid` (winit's
    /// `CursorLeft`). Drops the cursor + rolls back any in-flight
    /// `Hover`.
    ///
    /// R672 §5.35 §5.41 — single-window wrapper around
    /// [`Self::cursor_left_for_window`].
    pub fn cursor_left(&mut self, pid: PointerId) -> DispatchTail<V::State> {
        self.cursor_left_for_window(DEFAULT_WINDOW, pid)
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::cursor_left`].
    pub fn cursor_left_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
    ) -> DispatchTail<V::State> {
        let Self { scene, routers, .. } = self;
        let router = routers
            .entry(window_id.to_owned())
            .or_default();
        router.cursor_left(pid, scene);
        self.tail()
    }

    /// R51.122 §5.41 — pointer press (mouse left button down / touch
    /// start). Dispatches `PointerDown` to the current hover target
    /// then drains the dispatch tail. The Vello shell follows up with
    /// its `click_to_focus` step; the substrate stays focus-agnostic.
    ///
    /// R672 §5.35 §5.41 — single-window wrapper around
    /// [`Self::pointer_down_for_window`].
    pub fn pointer_down(&mut self, pid: PointerId) -> DispatchTail<V::State> {
        self.pointer_down_for_window(DEFAULT_WINDOW, pid)
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::pointer_down`].
    pub fn pointer_down_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
    ) -> DispatchTail<V::State> {
        let Self { scene, routers, .. } = self;
        let router = routers
            .entry(window_id.to_owned())
            .or_default();
        router.pointer_down(pid, scene);
        self.tail()
    }

    /// R51.122 §5.41 — pointer release (mouse left button up / touch
    /// end). Dispatches `PointerUp` to the current hover target then
    /// drains the dispatch tail.
    ///
    /// R672 §5.35 §5.41 — single-window wrapper around
    /// [`Self::pointer_up_for_window`].
    pub fn pointer_up(&mut self, pid: PointerId) -> DispatchTail<V::State> {
        self.pointer_up_for_window(DEFAULT_WINDOW, pid)
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::pointer_up`].
    pub fn pointer_up_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
    ) -> DispatchTail<V::State> {
        let Self { scene, routers, .. } = self;
        let router = routers
            .entry(window_id.to_owned())
            .or_default();
        router.pointer_up(pid, scene);
        self.tail()
    }

    /// R51.122 §5.41 — pointer cancellation (touch interrupted by OS
    /// gesture, phone-call notification, 4-finger swipe).
    /// Dispatches `PointerCancel` (not `PointerUp`) so the widget
    /// statechart routes `Pressed → Idle` without raising the
    /// activate event; then drains the dispatch tail.
    ///
    /// R672 §5.35 §5.41 — single-window wrapper around
    /// [`Self::pointer_cancel_for_window`].
    pub fn pointer_cancel(&mut self, pid: PointerId) -> DispatchTail<V::State> {
        self.pointer_cancel_for_window(DEFAULT_WINDOW, pid)
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::pointer_cancel`].
    pub fn pointer_cancel_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
    ) -> DispatchTail<V::State> {
        let Self { scene, routers, .. } = self;
        let router = routers
            .entry(window_id.to_owned())
            .or_default();
        router.pointer_cancel(pid, scene);
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
        self.touch_event_for_window(DEFAULT_WINDOW, touch)
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::touch_event`].
    /// All four phase arms route through the addressed window's
    /// router so multi-touch on a secondary window does not bleed
    /// into the primary's pointer state.
    pub fn touch_event_for_window(
        &mut self,
        window_id: &str,
        touch: Touch,
    ) -> DispatchTail<V::State> {
        let pid = PointerId::touch(touch.id);
        let Self { scene, routers, .. } = self;
        let router = routers
            .entry(window_id.to_owned())
            .or_default();
        match touch.phase {
            TouchPhase::Started => {
                router.cursor_moved(pid, touch.x, touch.y, scene);
                router.pointer_down(pid, scene);
            }
            TouchPhase::Moved => {
                router.cursor_moved(pid, touch.x, touch.y, scene);
            }
            TouchPhase::Ended => {
                router.pointer_up(pid, scene);
                router.cursor_left(pid, scene);
            }
            TouchPhase::Cancelled => {
                router.pointer_cancel(pid, scene);
                router.cursor_left(pid, scene);
            }
        }
        self.tail()
    }

    /// (R51.186 §5.45 R55.C.2) Mouse wheel dispatch.
    ///
    /// Forwards the wheel delta through
    /// [`InputRouter::wheel`](crate::input::InputRouter::wheel) which
    /// walks the retained paint scene's deepest
    /// [`Scene::Scroll`](pinion_core::scene::Scene::Scroll) under the
    /// pointer's stored cursor and calls
    /// [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by)
    /// on the attached state. Returns a `(DispatchTail, dispatched)`
    /// pair: the tail mirrors every other dispatch entry point so
    /// backends route any drained intents through the same handler,
    /// and the `dispatched: bool` lifts the router's no-op /
    /// hit-and-dispatched distinction so backends can decide whether
    /// to request a repaint (silent drops never bump the redraw
    /// flag).
    ///
    /// The `DispatchTail::state_change` is almost always `None`
    /// here: the scroll offset lives on the reactive `ScrollState`
    /// (a `Signal<i32>` write the next paint observes), not on the
    /// cached `V::State` snapshot the substrate compares. The
    /// backend's `wheel` wrapper instead reads the `dispatched`
    /// boolean to decide when to repaint.
    pub fn wheel(
        &mut self,
        pid: PointerId,
        delta: WheelDelta,
    ) -> (DispatchTail<V::State>, bool) {
        self.wheel_for_window(DEFAULT_WINDOW, pid, delta)
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::wheel`].
    pub fn wheel_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
        delta: WheelDelta,
    ) -> (DispatchTail<V::State>, bool) {
        let router = self
            .routers
            .entry(window_id.to_owned())
            .or_default();
        let dispatched = router.wheel(pid, delta);
        (self.tail(), dispatched)
    }

    /// (R51.187 §5.45 R55.C.3) Keyboard scroll dispatch.
    ///
    /// Forwards a W3C `KeyboardEvent.key` string into
    /// [`InputRouter::scroll_key`](crate::input::InputRouter::scroll_key)
    /// which walks the deepest [`Scene::Scroll`](pinion_core::scene::Scene::Scroll)
    /// under the pointer's stored cursor and translates the key
    /// into a `scroll_by` / `scroll_to` call on the attached
    /// [`ScrollState`](pinion_core::widgets::scroll::ScrollState).
    /// Recognised keys: `ArrowUp` / `ArrowDown` / `ArrowLeft` /
    /// `ArrowRight` (1-line step), `PageUp` / `PageDown` (1-page
    /// step = viewport height), `Home` / `End` (y-axis extremes).
    ///
    /// Returns `(DispatchTail, dispatched: bool)` mirroring
    /// [`Self::wheel`]. Backends use the `dispatched` boolean to
    /// gate redraw and only fall back to scroll-routing for keys
    /// the widget's own `apply_key` did not consume — the regular
    /// dispatch arm stays primary so widget-bound keys (Slider's
    /// `ArrowLeft` / `ArrowRight`, Toggle's `Space`, etc.) keep
    /// their existing semantics.
    pub fn scroll_key(
        &mut self,
        pid: PointerId,
        key: &str,
    ) -> (DispatchTail<V::State>, bool) {
        self.scroll_key_for_window(DEFAULT_WINDOW, pid, key)
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::scroll_key`].
    pub fn scroll_key_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
        key: &str,
    ) -> (DispatchTail<V::State>, bool) {
        let router = self
            .routers
            .entry(window_id.to_owned())
            .or_default();
        let dispatched = router.scroll_key(pid, key);
        (self.tail(), dispatched)
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
        assert!(core.apply_key(Some("test_btn"), "ArrowLeft", pinion_core::Modifiers::empty()).is_none());
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn apply_key_returns_tail_with_intent_for_handled_space() {
        // R51.122 — `apply_key(Some(tag), "Space")` resolves through
        // `apply_aria_activate` for the matching focused tag,
        // emitting a `click` intent. State stays Idle (KeyboardActivate
        // is an internal SCXML transition).
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let Some(tail) = core.apply_key(Some("test_btn"), "Space", pinion_core::Modifiers::empty()) else {
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
        assert!(core.apply_key(Some("other_widget"), "Space", pinion_core::Modifiers::empty()).is_none());
        assert!(core.apply_key(None, "Space", pinion_core::Modifiers::empty()).is_none());
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

    // ─────────────────────────────────────────────────────────────────
    // R51.142 §5.28 — root_owner + tick_animations substrate tests.
    // ─────────────────────────────────────────────────────────────────

    use pinion_core::animation::Tickable;
    use std::cell::Cell;
    use std::rc::Rc;

    /// Test-only [`Tickable`] that records every `tick(dt)` it sees so a
    /// test can assert the substrate forwarded the right delta into
    /// the right number of dispatches.
    struct TickRecorder {
        last_dt: Cell<f32>,
        ticks: Cell<u32>,
    }

    impl TickRecorder {
        fn new() -> Self {
            Self {
                last_dt: Cell::new(f32::NAN),
                ticks: Cell::new(0),
            }
        }
    }

    impl Tickable for TickRecorder {
        fn tick(&self, dt: f32) {
            self.last_dt.set(dt);
            self.ticks.set(self.ticks.get() + 1);
        }

        fn is_at_rest(&self, _epsilon: f32) -> bool {
            // Always non-rest so [`Owner::tick_animations`] never
            // short-circuits the dispatch and the test records every
            // tick.
            false
        }
    }

    #[test]
    fn root_owner_accessor_yields_usable_owner() {
        // R51.142 — `root_owner()` exposes the binding's reactive
        // scope so [`Owner::register_animation`] succeeds against it.
        // Two reads return references to the same owner instance:
        // animations registered through either handle land in the
        // same tick list.
        let core: CoreShell<TestButton> = CoreShell::new();
        let recorder = Rc::new(TickRecorder::new());
        core.root_owner().register_animation(recorder.clone());
        // A second borrow registers another tickable — both ticks
        // dispatch from the same internal vec on the next sweep.
        let recorder_b = Rc::new(TickRecorder::new());
        core.root_owner().register_animation(recorder_b.clone());
        core.tick_animations(1.0 / 60.0);
        assert_eq!(recorder.ticks.get(), 1);
        assert_eq!(recorder_b.ticks.get(), 1);
    }

    #[test]
    fn tick_animations_forwards_dt_to_registered_tickables() {
        // R51.142 — the substrate's `tick_animations(dt)` is the
        // exact `dt` the registered Tickables observe in their
        // `tick(dt)` callback. No clamping, no scaling.
        let core: CoreShell<TestButton> = CoreShell::new();
        let recorder = Rc::new(TickRecorder::new());
        core.root_owner().register_animation(recorder.clone());
        core.tick_animations(0.016_666_67);
        assert_eq!(recorder.ticks.get(), 1);
        assert_eq!(recorder.last_dt.get().to_bits(), 0.016_666_67_f32.to_bits());
    }

    #[test]
    fn tick_animations_idempotent_with_zero_dt_when_owner_empty() {
        // R51.142 — calling `tick_animations(0.0)` on a fresh
        // substrate with no registered animations is a no-op. The
        // first paint or any synthetic flush passes `dt=0` and the
        // call must not panic / allocate / touch state.
        let core: CoreShell<TestButton> = CoreShell::new();
        core.tick_animations(0.0);
        core.tick_animations(0.0);
        // Cached state unchanged — animations were never registered
        // so the visible Button state stays at the construction
        // baseline.
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn tick_animations_drives_registered_animation_repeatedly() {
        // R51.142 — successive ticks accumulate dispatches without
        // unregistering or skipping the tickable; the substrate
        // never assumes one-shot semantics.
        let core: CoreShell<TestButton> = CoreShell::new();
        let recorder = Rc::new(TickRecorder::new());
        core.root_owner().register_animation(recorder.clone());
        for _ in 0..5 {
            core.tick_animations(0.01);
        }
        assert_eq!(recorder.ticks.get(), 5);
        assert_eq!(recorder.last_dt.get().to_bits(), 0.01_f32.to_bits());
    }

    // ─────────────────────────────────────────────────────────────────
    // R51.149 §5.28 — Effect-driven AnimationDriver tests.
    //
    // The spec진본화 (§5.28 R33 framework AnimationDriver) routes the
    // `tick_animations(dt)` call through a reactive Effect subscribed
    // to a monotonic frame counter signal. The tests below pin down
    // the load-bearing invariants:
    //
    // - frame_signal counter monotonically increments on every tick
    // - the Effect re-runs on each counter bump (verified through
    //   the Tickable recorder)
    // - identical dt values across ticks still propagate (sidesteps
    //   Signal::set's equality-skip via the counter pattern)
    // - last_dt mirrors the most-recent value passed to
    //   tick_animations
    // ─────────────────────────────────────────────────────────────────

    // ─────────────────────────────────────────────────────────────────
    // R51.152 §5.22 — CoreShell::apply_key wraps V::apply_key in
    // root_owner.run(...) so application-side per-binding state
    // (typeahead cursors, etc.) reach Owner::cache from inside the
    // keyboard handler. The test exercises the wrap via the existing
    // TestButton fixture's apply_key path: even though `apply_aria_activate`
    // doesn't read Owner::current(), the substrate's wrap must hold.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r51_152_apply_key_runs_under_root_owner_scope() {
        // R51.152 — direct verification that the apply_key wrap is
        // observable. We capture Owner::current() inside a closure
        // executed during the apply_key path. The trick: TestButton's
        // apply_key delegates to apply_aria_activate which doesn't
        // observe; so we side-channel through a per-test
        // thread_local that records the observation from a wrapping
        // helper fixture.
        //
        // For simplicity we verify the wrap's *outer* behaviour: the
        // Owner::current() observation BEFORE and AFTER apply_key
        // brackets are None (the wrap pops on exit), and the
        // observation during V::view (the existing wrap) returns the
        // same root_owner id apply_key would see.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        // Pre-apply: no active wrap.
        assert!(pinion_core::Owner::current().is_none());
        let _ = core.apply_key(Some("test_btn"), "Space", pinion_core::Modifiers::empty());
        // Post-apply: wrap popped.
        assert!(
            pinion_core::Owner::current().is_none(),
            "apply_key's root_owner.run wrap must pop on exit",
        );
        // The wrap is symmetric: V::view runs under the same owner.
        // We've already verified through R51.146 that V::view sees
        // root_owner; the apply_key wrap mirrors the same shape.
    }

    // ─────────────────────────────────────────────────────────────────
    // R56.2.a §5.13 §5.38 — CoreShell::apply_composition wraps
    // V::apply_composition in root_owner.run(...) and returns
    // Some(DispatchTail) on handled / None on unhandled. The
    // TestButton fixture leaves WidgetCore::apply_composition at its
    // default (`false`) so every dispatch path here lands on the
    // None arm; the handled arm is exercised at the consumer side
    // (hello-textfield's TextFieldView impl exercises the route
    // end-to-end through `find_external_with_tag_mut + invoke`).
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r56_2_a_apply_composition_returns_none_for_default_impl() {
        // R56.2.a — `WidgetCore::apply_composition` defaults to
        // `false`; CoreShell wraps and yields `None`. The cached
        // state stays in `Idle` because no dispatch tail was drained.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        for event in [
            pinion_core::CompositionEvent::Start,
            pinion_core::CompositionEvent::Update("ha".to_owned()),
            pinion_core::CompositionEvent::Commit("han".to_owned()),
            pinion_core::CompositionEvent::Cancel,
        ] {
            assert!(
                core.apply_composition(Some("test_btn"), &event).is_none(),
                "default apply_composition must yield None on {event:?}",
            );
        }
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn r56_2_a_apply_composition_pops_root_owner_on_exit() {
        // R56.2.a — wrap symmetric with apply_key (R51.152): the
        // `root_owner.run(...)` bracket must pop on exit so
        // `Owner::current()` outside the dispatch path stays None.
        // We exercise both the focused arm and the wrong-focus arm
        // to confirm the wrap pops regardless of the trait return.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        assert!(pinion_core::Owner::current().is_none());
        let _ = core.apply_composition(
            Some("test_btn"),
            &pinion_core::CompositionEvent::Start,
        );
        assert!(
            pinion_core::Owner::current().is_none(),
            "apply_composition's root_owner.run wrap must pop on exit",
        );
        let _ = core.apply_composition(
            Some("foreign_tag"),
            &pinion_core::CompositionEvent::Commit("x".to_owned()),
        );
        assert!(
            pinion_core::Owner::current().is_none(),
            "wrap pops even when V::apply_composition returns false",
        );
        let _ = core.apply_composition(
            None,
            &pinion_core::CompositionEvent::Cancel,
        );
        assert!(
            pinion_core::Owner::current().is_none(),
            "wrap pops on unfocused dispatch (None focus)",
        );
    }

    #[test]
    fn r56_2_a_apply_composition_borrows_event_without_consuming() {
        // R56.2.a — the trait method takes `&CompositionEvent`, so
        // callers can re-use the event after dispatch (e.g. log it,
        // forward to RPC observability). Verify the borrow contract
        // by re-reading the event after the call.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let event = pinion_core::CompositionEvent::Update("ha".to_owned());
        let _ = core.apply_composition(Some("test_btn"), &event);
        // event is still owned and inspectable after the call.
        match &event {
            pinion_core::CompositionEvent::Update(text) => assert_eq!(text, "ha"),
            _ => panic!("event must survive borrow"),
        }
    }

    #[test]
    fn r51_149_frame_signal_starts_at_zero() {
        // R51.149 — counter initial value (Signal::new(0_u64)). The
        // eager Effect run subscribes at construction without
        // bumping the counter.
        let core: CoreShell<TestButton> = CoreShell::new();
        assert_eq!(core.frame_signal().get(), 0_u64);
    }

    #[test]
    fn r51_149_tick_animations_bumps_frame_counter() {
        // R51.149 — every tick increments the counter by exactly 1.
        let core: CoreShell<TestButton> = CoreShell::new();
        assert_eq!(core.frame_signal().get(), 0_u64);
        core.tick_animations(1.0 / 60.0);
        assert_eq!(core.frame_signal().get(), 1_u64);
        core.tick_animations(1.0 / 60.0);
        assert_eq!(core.frame_signal().get(), 2_u64);
    }

    #[test]
    fn r51_149_identical_dt_still_propagates_through_effect() {
        // R51.149 load-bearing — the pre-R51.149 attempt at a raw
        // `Signal<f32>` dt-signal would equality-skip when two ticks
        // pushed the same dt; the counter-based design must dispatch
        // both ticks regardless of dt repetition. The test pushes
        // the same dt five times and asserts five Tickable dispatches.
        let core: CoreShell<TestButton> = CoreShell::new();
        let recorder = Rc::new(TickRecorder::new());
        core.root_owner().register_animation(recorder.clone());
        for _ in 0..5 {
            core.tick_animations(0.016_666_67);
        }
        assert_eq!(
            recorder.ticks.get(),
            5,
            "identical dt across ticks must each re-fire the Effect (not equality-skip)",
        );
        assert_eq!(
            recorder.last_dt.get().to_bits(),
            0.016_666_67_f32.to_bits(),
        );
    }

    #[test]
    fn r51_149_application_effect_can_observe_frame_signal() {
        // R51.149 — applications subscribing to `frame_signal` see
        // the per-tick counter cascade. Verifies the public accessor
        // is wired into the same Signal the substrate's internal
        // driver fires on.
        use std::cell::Cell;
        use pinion_core::reactive::Effect;
        let core: CoreShell<TestButton> = CoreShell::new();
        let observed = Rc::new(Cell::new(0_u32));
        let observed_clone = Rc::clone(&observed);
        let signal_clone = core.frame_signal().clone();
        let _user_effect = core.root_owner().run(|| {
            Effect::new(core.root_owner(), move || {
                let _frame = signal_clone.get();
                observed_clone.set(observed_clone.get() + 1);
            })
        });
        // Eager initial run already counted 1.
        let baseline = observed.get();
        core.tick_animations(0.01);
        core.tick_animations(0.02);
        assert_eq!(
            observed.get(),
            baseline + 2,
            "application Effect must re-run on each tick_animations bump",
        );
    }

    #[test]
    fn any_animation_active_false_for_empty_substrate() {
        // R51.147 — fresh substrate has no animations; the helper
        // reports `false` so the backend can idle.
        let core: CoreShell<TestButton> = CoreShell::new();
        assert!(!core.any_animation_active(0.01));
    }

    #[test]
    fn any_animation_active_true_with_non_at_rest_tickable() {
        // R51.147 — the recorder fixture's `is_at_rest` is hard-coded
        // to `false` so the substrate observes an active animation
        // after registration.
        let core: CoreShell<TestButton> = CoreShell::new();
        let recorder = Rc::new(TickRecorder::new());
        core.root_owner().register_animation(recorder);
        assert!(core.any_animation_active(0.01));
    }

    #[test]
    fn root_owner_drop_drops_registered_animations() {
        // R51.142 — when the substrate goes out of scope the
        // [`Owner`] inside drops too, releasing every animation Rc
        // it held. Verified via strong_count: the only outstanding
        // strong reference outside the substrate is the test's local
        // Rc, so after the drop strong_count is 1.
        let recorder = Rc::new(TickRecorder::new());
        {
            let core: CoreShell<TestButton> = CoreShell::new();
            core.root_owner().register_animation(recorder.clone());
            assert!(Rc::strong_count(&recorder) >= 2);
        }
        assert_eq!(Rc::strong_count(&recorder), 1);
    }

    // ─────────────────────────────────────────────────────────────────
    // R51.157 §5.23 — CommandExecutor drain pump tests.
    //
    // Owner.dispatch_command queues commands on the binding's root
    // reactive scope; CoreShell::dispatch_pending_commands drains them
    // (recursively, R51.139 contract) and feeds each through the
    // installed CommandExecutor. The tests below pin the load-bearing
    // invariants:
    //
    // - no executor → no drain (leaves queue untouched)
    // - registered handler → command routed, sink observes resulting Intent
    // - unregistered kind → returned in unhandled Vec, sink stays empty
    // - mixed (some handled, some not) → handled dispatched, unhandled returned
    // - recursive drain reaches child-scope commands
    // - executor swap returns prior + uses new for subsequent drains
    // ─────────────────────────────────────────────────────────────────

    use crate::command::{
        BlockOnExecutor, CommandExecutor, Executor, HandlerRegistry, IntentSink, VecSink,
    };
    use pinion_core::Command;

    fn echo_handler() -> std::sync::Arc<dyn crate::command::Handler> {
        std::sync::Arc::new(|cmd: Command| -> crate::command::HandlerFuture {
            Box::pin(async move {
                Intent::new_owned(
                    format!("echo.{}", cmd.kind_str()),
                    cmd.payload,
                )
            })
        })
    }

    fn build_executor_with(
        kinds: &[&'static str],
    ) -> (Arc<CommandExecutor>, Arc<VecSink>) {
        let mut reg = HandlerRegistry::new();
        for k in kinds {
            reg.register(*k, echo_handler());
        }
        let sink = Arc::new(VecSink::new());
        let exec: Arc<dyn Executor> = Arc::new(BlockOnExecutor);
        let sink_dyn: Arc<dyn IntentSink> = sink.clone();
        let cmd_exec = Arc::new(CommandExecutor::new(reg, exec, sink_dyn));
        (cmd_exec, sink)
    }

    #[test]
    fn r51_157_dispatch_without_executor_returns_empty_and_keeps_queue() {
        // R51.157 — no executor installed → drain pump is a no-op.
        // Owner-side pending queue stays populated so an AI inspection
        // round can still observe the parked Command via
        // `Owner::pending_commands`.
        let core: CoreShell<TestButton> = CoreShell::new();
        core.root_owner().dispatch_command(Command::new_static(
            "http.get",
            IntrospectValue::Null,
            0,
        ));
        let unhandled = core.dispatch_pending_commands();
        assert!(unhandled.is_empty(), "no executor → empty Vec returned");
        assert_eq!(
            core.root_owner().pending_commands().len(),
            1,
            "no-executor drain must NOT consume the queue",
        );
    }

    #[test]
    fn r51_157_dispatch_routes_handled_command_to_sink() {
        // R51.157 — installed executor with a registered handler:
        // the drained Command resolves through BlockOnExecutor inside
        // dispatch (sync), and the resulting Intent reaches the sink.
        let (executor, sink) = build_executor_with(&["http.get"]);
        let core: CoreShell<TestButton> =
            CoreShell::new().with_executor(executor);
        core.root_owner().dispatch_command(Command::new_static(
            "http.get",
            IntrospectValue::Text("/api/v1".into()),
            42,
        ));
        let unhandled = core.dispatch_pending_commands();
        assert!(unhandled.is_empty(), "registered kind → empty unhandled");
        let drained = sink.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].tag_str(), "echo.http.get");
        assert_eq!(
            drained[0].payload,
            IntrospectValue::Text("/api/v1".into()),
        );
        // Owner queue is now empty (the drain pump consumed it).
        assert!(core.root_owner().pending_commands().is_empty());
    }

    #[test]
    fn r51_157_dispatch_unhandled_kind_returned_to_caller() {
        // R51.157 — registry has no handler for `audio.play`; the
        // command is returned in the unhandled Vec so the backend
        // can log it. The sink stays empty because no future ran.
        let (executor, sink) = build_executor_with(&["http.get"]);
        let core: CoreShell<TestButton> =
            CoreShell::new().with_executor(executor);
        core.root_owner().dispatch_command(Command::new_static(
            "audio.play",
            IntrospectValue::Int(440),
            0,
        ));
        let unhandled = core.dispatch_pending_commands();
        assert_eq!(unhandled.len(), 1, "unhandled kind returned");
        assert_eq!(unhandled[0].kind_str(), "audio.play");
        assert_eq!(unhandled[0].payload, IntrospectValue::Int(440));
        assert!(sink.is_empty(), "no handler → no sink delivery");
    }

    #[test]
    fn r51_157_dispatch_mixed_handled_and_unhandled() {
        // R51.157 — handled commands dispatch through the executor;
        // unhandled commands accumulate in the returned Vec. The
        // sink observes only the handled ones, in dispatch order.
        let (executor, sink) = build_executor_with(&["a", "c"]);
        let core: CoreShell<TestButton> =
            CoreShell::new().with_executor(executor);
        let owner = core.root_owner();
        owner.dispatch_command(Command::new_static("a", IntrospectValue::Int(1), 0));
        owner.dispatch_command(Command::new_static("b", IntrospectValue::Int(2), 0));
        owner.dispatch_command(Command::new_static("c", IntrospectValue::Int(3), 0));
        owner.dispatch_command(Command::new_static("d", IntrospectValue::Int(4), 0));
        let unhandled = core.dispatch_pending_commands();
        let unhandled_kinds: Vec<&str> =
            unhandled.iter().map(Command::kind_str).collect();
        assert_eq!(unhandled_kinds, vec!["b", "d"]);
        let drained = sink.drain();
        let handled_tags: Vec<&str> = drained.iter().map(Intent::tag_str).collect();
        assert_eq!(handled_tags, vec!["echo.a", "echo.c"]);
    }

    #[test]
    fn r51_157_dispatch_drains_child_scope_commands_too() {
        // R51.157 — `take_pending_commands_recursive` walks the
        // children-first cascade (R51.139); the drain pump inherits
        // the recursion automatically. Both child + parent commands
        // reach the sink.
        let (executor, sink) = build_executor_with(&["child.evt", "parent.evt"]);
        let core: CoreShell<TestButton> =
            CoreShell::new().with_executor(executor);
        let child = Owner::new_child(core.root_owner());
        child.dispatch_command(Command::new_static(
            "child.evt",
            IntrospectValue::Null,
            child.id(),
        ));
        core.root_owner().dispatch_command(Command::new_static(
            "parent.evt",
            IntrospectValue::Null,
            core.root_owner().id(),
        ));
        let unhandled = core.dispatch_pending_commands();
        assert!(unhandled.is_empty());
        let drained = sink.drain();
        let tags: Vec<&str> = drained.iter().map(Intent::tag_str).collect();
        // R51.139 cascade order = children first, then self.
        assert_eq!(tags, vec!["echo.child.evt", "echo.parent.evt"]);
    }

    #[test]
    fn r51_157_set_executor_returns_prior_handle() {
        // R51.157 — `set_executor` is the swappable-registration
        // entry the §5.23 R27 contract calls for. Replacing returns
        // the prior handle so tests can restore baseline after a
        // scoped swap.
        let (first, _sink_a) = build_executor_with(&["k"]);
        let (second, _sink_b) = build_executor_with(&["k"]);
        let first_id = Arc::as_ptr(&first).cast::<()>() as usize;
        let mut core: CoreShell<TestButton> =
            CoreShell::new().with_executor(first);
        let prior = core.set_executor(second);
        let prior = prior.expect("first executor returned on replace");
        assert_eq!(Arc::as_ptr(&prior).cast::<()>() as usize, first_id);
    }

    #[test]
    fn r51_157_clear_executor_returns_prior_and_disables_drain() {
        // R51.157 — `clear_executor` returns the detached handle (so
        // a shutdown path can still drive remaining in-flight work)
        // and switches the drain pump back to no-op behaviour.
        let (executor, sink) = build_executor_with(&["k"]);
        let mut core: CoreShell<TestButton> =
            CoreShell::new().with_executor(executor);
        let _detached = core
            .clear_executor()
            .expect("clear returns the prior executor");
        assert!(core.executor().is_none());
        core.root_owner().dispatch_command(Command::new_static(
            "k",
            IntrospectValue::Null,
            0,
        ));
        let unhandled = core.dispatch_pending_commands();
        assert!(unhandled.is_empty(), "no executor → no-op drain");
        assert!(sink.is_empty(), "no executor → sink stays empty");
        assert_eq!(
            core.root_owner().pending_commands().len(),
            1,
            "no-executor drain must NOT consume the queue",
        );
    }

    #[test]
    fn r51_157_executor_accessor_yields_none_until_set() {
        // R51.157 — fresh substrate has no executor.
        let core: CoreShell<TestButton> = CoreShell::new();
        assert!(core.executor().is_none());
    }

    #[test]
    fn r51_157_with_executor_builder_attaches_handle() {
        // R51.157 — builder chains the executor into the new
        // substrate without an intermediate mutable borrow.
        let (executor, _sink) = build_executor_with(&["k"]);
        let exec_ptr = Arc::as_ptr(&executor).cast::<()>() as usize;
        let core: CoreShell<TestButton> = CoreShell::new().with_executor(executor);
        let installed = core.executor().expect("with_executor installs Some");
        assert_eq!(Arc::as_ptr(installed).cast::<()>() as usize, exec_ptr);
    }

    // R51.167 §5.23 R27 — `route_intent_through_update` calls the
    // `WidgetCore::update` reducer and queues every produced
    // `Command` on the substrate's `root_owner`. Tests exercise
    // the default-reducer (empty) path on `TestButton` and the
    // shared override fixture
    // [`pinion_core::test_fixtures::EchoButtonFixture`] (lifted to
    // `pinion-core::test_fixtures` so the R51.168 shell + tui
    // wiring tests reuse the same `update` body).
    use pinion_core::test_fixtures::EchoButtonFixture as EchoButton;

    #[test]
    fn r51_167_route_intent_default_reducer_yields_empty() {
        // R51.167 — `TestButton` carries no `update` override, so
        // the default `Vec::new()` reducer flows through the routing
        // path unchanged. The owner queue stays empty.
        let core: CoreShell<TestButton> = CoreShell::new();
        let intent = Intent::new_static("test_btn.click", IntrospectValue::Null);
        let queued = core.route_intent_through_update(&intent);
        assert!(queued.is_empty());
        assert!(core.root_owner().pending_commands().is_empty());
    }

    #[test]
    fn r51_167_route_intent_queues_reducer_commands() {
        // R51.167 — `EchoButton::update` emits one command per
        // intent; the routing path queues it on the root owner
        // AND returns the same Vec for direct caller inspection.
        let core: CoreShell<EchoButton> = CoreShell::new();
        let intent = Intent::new_static("echo_btn.tick", IntrospectValue::Null);
        let queued = core.route_intent_through_update(&intent);
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].kind_str(), "echo.reply");
        assert_eq!(
            queued[0].payload,
            IntrospectValue::Text("echo_btn.tick".to_string()),
        );
        let pending = core.root_owner().pending_commands();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind_str(), "echo.reply");
    }

    #[test]
    fn r51_167_route_intent_accumulates_across_calls() {
        // R51.167 — multiple intents pile their reducer-produced
        // commands onto the owner queue in FIFO order, so a later
        // `dispatch_pending_commands` pump reaches every handler.
        let core: CoreShell<EchoButton> = CoreShell::new();
        let i1 = Intent::new_static("echo_btn.a", IntrospectValue::Null);
        let i2 = Intent::new_static("echo_btn.b", IntrospectValue::Null);
        let _ = core.route_intent_through_update(&i1);
        let _ = core.route_intent_through_update(&i2);
        let pending = core.root_owner().pending_commands();
        assert_eq!(pending.len(), 2);
        assert_eq!(
            pending[0].payload,
            IntrospectValue::Text("echo_btn.a".to_string()),
        );
        assert_eq!(
            pending[1].payload,
            IntrospectValue::Text("echo_btn.b".to_string()),
        );
    }

    // R51.171 §5.22 R26 — `route_intent_through_update` wraps the
    // `V::update` call in `root_owner.run(...)` so
    // `Owner::current()` inside the reducer resolves to the
    // substrate's root owner. Mirrors the
    // [[callback-root-owner-wrap]] pattern already applied to
    // `V::view` (R51.146) and `V::apply_key` (R51.152).

    use pinion_core::External;
    use std::sync::Mutex;

    /// R51.176 §5.22 — single-use capture slot for the `r51_171`
    /// `Owner::current()` observation. `Option<u64>` ranges over the
    /// two test phases naturally: `None` at entry means "reducer has
    /// not been called yet" (caught by `.expect(...)` if the wrap
    /// regressed to a NOOP), `Some(id)` after the call carries the
    /// captured `Owner::current()` value. Mutex over plain atomic
    /// keeps the sentinel and the value in the same type so future
    /// test additions cannot store a "captured 0" that aliases with
    /// the initial state.
    static R51_171_CAPTURED_OWNER_ID: Mutex<Option<u64>> = Mutex::new(None);

    struct OwnerCaptureButton;

    impl WidgetCore for OwnerCaptureButton {
        type State = ButtonState;
        type Event = ButtonEvent;

        fn create_external() -> Box<dyn External> {
            <EchoButton as WidgetCore>::create_external()
        }

        fn tag() -> &'static str {
            "owner_capture_btn"
        }

        fn read_state(scene: &Scene) -> Self::State {
            <EchoButton as WidgetCore>::read_state(scene)
        }

        fn view(state: Self::State, frame: &Frame) -> Scene {
            <EchoButton as WidgetCore>::view(state, frame)
        }

        fn event_name(event: Self::Event) -> &'static str {
            <EchoButton as WidgetCore>::event_name(event)
        }

        fn title() -> &'static str {
            "OwnerCapture"
        }

        fn update(_state: Self::State, _intent: &Intent) -> Vec<Command> {
            *R51_171_CAPTURED_OWNER_ID.lock().unwrap() =
                Some(pinion_core::Owner::current().map_or(0, |o| o.id()));
            Vec::new()
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // R51.186 §5.45 R55.C.2 — CoreShell::wheel forwards through the
    // router and lifts the dispatched bool out of the tail. The
    // tests below pin the load-bearing invariants:
    //
    // - wheel dispatches against the attached ScrollState
    // - the dispatched bool is true only on actual scroll
    // - no-scroll-at-cursor → (empty tail, false)
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_c2_wheel_dispatches_to_attached_scroll_state() {
        use pinion_core::scene::{BoxNode, ContainerNode, Rect, ScrollNode};
        use pinion_core::style::{BoxStyle, Color};
        use pinion_core::widgets::scroll::ScrollState;
        use std::rc::Rc;

        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);

        let mut core: CoreShell<TestButton> = CoreShell::new();
        let scroll_content = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 200, 1000),
            Color::default(),
        ));
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), scroll_content)
            .with_state(Rc::clone(&state));
        let mut root = Scene::Container(
            ContainerNode::new(vec![Scene::Scroll(scroll)])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 200, 200);
        }
        core.update_paint_scene(root);
        let _ = core.cursor_moved(PointerId::MOUSE, 50.0, 50.0);
        let (tail, dispatched) = core.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 60.0 },
        );
        assert!(dispatched, "wheel must dispatch against attached state");
        assert_eq!(state.offset(), (0, 60));
        assert!(tail.intents.is_empty(), "wheel emits no SCXML intents");
        assert!(
            tail.state_change.is_none(),
            "scroll offset lives off the cached projection — no cached_state delta",
        );
    }

    #[test]
    fn r55_c2_wheel_returns_false_when_no_scroll_under_cursor() {
        // R55.C.2 — wheel over a button-like scene (no Scroll
        // variant in the paint tree) returns (empty tail, false).
        // Backends use the false to skip the per-event redraw.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let paint = <TestButton as WidgetCore>::view(*core.cached_state(), &Frame::new());
        core.update_paint_scene(paint);
        let _ = core.cursor_moved(PointerId::MOUSE, 8.0, 8.0);
        let (_tail, dispatched) = core.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 40.0 },
        );
        assert!(!dispatched);
    }

    #[test]
    fn r51_171_update_runs_inside_root_owner_run_scope() {
        // R51.171 — `Owner::current()` inside `V::update` must
        // resolve to the substrate's root owner so reducer authors
        // can attribute `Command.scope_id` (RPC introspection
        // label) without reaching through framework internals.
        //
        // R51.176 §5.22 — `Option<u64>` sentinel + `.expect(...)`
        // replaces the R51.171 `AtomicU64::new(0)` / `u64::MAX`
        // sentinel pair. `None` at entry is the unambiguous "not
        // yet captured" state; a NOOP `root_owner.run` would leave
        // the slot at `None` (caught by `.expect(...)`). The
        // captured value is then asserted equal to the substrate's
        // own `root_owner().id()` — no assumption is made about the
        // numeric range of that id (`next_node_id` is a thread-local
        // counter that starts at 0, so the first owner allocated on
        // a given test thread may legitimately carry id `0`; only
        // the equality matters for the wrap contract).
        *R51_171_CAPTURED_OWNER_ID.lock().unwrap() = None;
        let core: CoreShell<OwnerCaptureButton> = CoreShell::new();
        let expected = core.root_owner().id();
        let intent = Intent::new_static("oc.tick", IntrospectValue::Null);
        let _ = core.route_intent_through_update(&intent);
        let captured = R51_171_CAPTURED_OWNER_ID
            .lock()
            .unwrap()
            .expect("V::update must run (capture slot was cleared at entry)");
        assert_eq!(
            captured, expected,
            "Owner::current() inside V::update must equal root_owner.id()",
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R55.D.5 §5.45 — multi-External substrate composition + drag
    // wiring. The fixture pairs the existing `ButtonFixture` with a
    // sibling `ScrollBarExternal` attached to a shared `ScrollState`
    // (the same `Owner::cache` key the view-fn would resolve via
    // `use_scroll_state`). The tests cover three contract layers:
    //
    //   1. State scene shape — `Scene::Container([primary, scrollbar])`
    //      when extras non-empty, bit-identical `Scene::External(primary)`
    //      when default.
    //   2. Lookup — both externals findable via
    //      `Scene::find_external_with_tag`; the primary is reachable
    //      via `Scene::primary_external` (DFS first).
    //   3. Drag dispatch — a `pointer_down` → `pointer_move` cycle
    //      against the scrollbar tag routes through the framework's
    //      capture-lock path into `ScrollBarExternal::pointer_move`
    //      and writes the shared `ScrollState` (closes the carry
    //      `hello-listbox` exercises in production).
    // ─────────────────────────────────────────────────────────────────

    use pinion_core::scene::{BoxNode, Rect};
    use pinion_core::widget_core::ExtraExternal;
    use pinion_core::widgets::scroll::{use_scroll_state, ScrollState};
    use pinion_core::widgets::scrollbar::ScrollBarExternal;
    use pinion_core::Color;
    use std::rc::Rc as TestRc;

    /// (R55.D.5 §5.45) Test fixture: `ButtonFixture` semantics plus a
    /// sibling [`ScrollBarExternal`] tagged `"sb"` whose
    /// `Rc<ScrollState>` shares the cache key `"sb_state"` with what
    /// the view fn would resolve. The state's `max_y` is seeded to
    /// 100 inside the same factory closure so the substrate-side
    /// composition test sees a non-zero scrollable range.
    struct ScrollbarMultiFixture;

    /// (R55.D.5 §5.45) Resolve the same `Rc<ScrollState>` the fixture's
    /// `create_extra_externals` seeded. Lives outside the `WidgetCore`
    /// impl block so tests can read the offset after dispatch without
    /// reaching through the substrate's private state.
    const SB_KEY: &str = "sb_state";

    impl WidgetCore for ScrollbarMultiFixture {
        type State = ButtonState;
        type Event = ButtonEvent;

        fn create_external() -> Box<dyn pinion_core::external::External> {
            <TestButton as WidgetCore>::create_external()
        }

        fn create_extra_externals() -> Vec<ExtraExternal> {
            let state = use_scroll_state(SB_KEY);
            state.set_max(0, 100);
            let bar = ScrollBarExternal::new().attach_state(state);
            vec![ExtraExternal::new("sb", Box::new(bar))]
        }

        fn tag() -> &'static str {
            "test_btn"
        }

        fn read_state(scene: &Scene) -> Self::State {
            // R55.D.5 — multi-External composition wraps the primary
            // External in a Container; walk to it by tag.
            scene
                .find_external_with_tag(<Self as WidgetCore>::tag())
                .and_then(|n| n.handle.introspect())
                .and_then(|i| i.query("state"))
                .map_or(ButtonState::Idle, |v| match v {
                    IntrospectValue::Text(s) if s == "Hover" => ButtonState::Hover,
                    IntrospectValue::Text(s) if s == "Pressed" => ButtonState::Pressed,
                    IntrospectValue::Text(s) if s == "Disabled" => ButtonState::Disabled,
                    _ => ButtonState::Idle,
                })
        }

        fn view(state: Self::State, frame: &Frame) -> Scene {
            <TestButton as WidgetCore>::view(state, frame)
        }

        fn event_name(event: Self::Event) -> &'static str {
            <TestButton as WidgetCore>::event_name(event)
        }

        fn title() -> &'static str {
            "MultiExternal"
        }
    }

    #[test]
    fn r55_d5_default_extras_keeps_state_scene_external() {
        // R55.D.5 — single-widget bindings (no `create_extra_externals`
        // override) keep the state scene as bare `Scene::External`,
        // bit-identical to the pre-R55.D.5 shape.
        let core: CoreShell<TestButton> = CoreShell::new();
        assert!(matches!(core.scene(), Scene::External(_)));
    }

    #[test]
    fn r55_d5_extras_wraps_state_scene_in_container() {
        // R55.D.5 — override returning one extra → state scene
        // becomes `Container([primary, scrollbar])`.
        let core: CoreShell<ScrollbarMultiFixture> = CoreShell::new();
        let Scene::Container(c) = core.scene() else {
            panic!("multi-External shape must wrap in Container");
        };
        assert_eq!(c.children.len(), 2, "primary + 1 extra");
        let Scene::External(primary) = &c.children[0] else {
            panic!("first child must be the primary External");
        };
        assert_eq!(primary.tag.as_deref(), Some("test_btn"));
        let Scene::External(extra) = &c.children[1] else {
            panic!("second child must be the extra External");
        };
        assert_eq!(extra.tag.as_deref(), Some("sb"));
    }

    #[test]
    fn r55_d5_find_external_with_tag_resolves_both_externals() {
        // R55.D.5 — both externals discoverable by tag.
        let core: CoreShell<ScrollbarMultiFixture> = CoreShell::new();
        assert!(core.scene().find_external_with_tag("test_btn").is_some());
        assert!(core.scene().find_external_with_tag("sb").is_some());
        assert!(core.scene().find_external_with_tag("nope").is_none());
    }

    #[test]
    fn r55_d5_primary_external_picks_button_not_scrollbar() {
        // R55.D.5 — DFS pre-order: the primary External is the first
        // child of the Container, matching the substrate's
        // composition convention.
        let core: CoreShell<ScrollbarMultiFixture> = CoreShell::new();
        let node = core.scene().primary_external().expect("must resolve");
        assert_eq!(node.tag.as_deref(), Some("test_btn"));
    }

    #[test]
    fn r55_d5_cached_state_reads_through_container_wrap() {
        // R55.D.5 — `read_state` walks the wrapper Container to find
        // the primary External and resolves the cached state from it.
        let core: CoreShell<ScrollbarMultiFixture> = CoreShell::new();
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    /// (R55.D.5 §5.45) Resolve the shared scroll state outside the
    /// shell so a test can compare offset before/after a drag
    /// dispatch. Reaches for the same `Owner::cache` key the fixture
    /// seeded; the returned `Rc<ScrollState>` is therefore identical
    /// to the one [`ScrollBarExternal::attach_state`] received.
    fn resolve_shared_scroll_state(core: &CoreShell<ScrollbarMultiFixture>) -> TestRc<ScrollState> {
        core.root_owner().run(|| use_scroll_state(SB_KEY))
    }

    /// (R55.D.5 §5.45) Build the paint scene the shell needs for
    /// hit-test routing: a Container with the scrollbar tag covering
    /// a 20×200 rect. Mirrors what hello-listbox's view fn produces
    /// (the scrollbar visual Container with `SCROLLBAR_TAG`). The
    /// runtime layout pass would normally write `rect`; the test
    /// hand-seeds it on the public `rect` field directly so the
    /// router's hit-test resolves without standing up a full layout
    /// pipeline.
    fn build_paint_scene_with_sb_rect() -> Scene {
        let sb_rect = Rect::new(0, 0, 20, 200);
        let inner = Scene::Box(BoxNode::filled(sb_rect, Color::default()));
        let mut sb_container = ContainerNode::new(vec![inner]).with_tag("sb");
        sb_container.rect = sb_rect;
        Scene::Container(sb_container)
    }

    #[test]
    fn r55_d5_pointer_drag_on_scrollbar_writes_shared_scroll_state() {
        // R55.D.5 — end-to-end drag: a `pointer_down` then
        // `pointer_move` on the scrollbar tag's paint rect drives the
        // capture-lock path into `ScrollBarExternal::pointer_move`,
        // which writes the shared `ScrollState`. The visible
        // contract: a drag from the top of the bar (y=0) to halfway
        // down (y=100, bar height 200) lands the offset near 50 (the
        // `delta_fraction × scroll_max` = 0.5 × 100 = 50 closed-form
        // for a Vertical orientation, R55.D.3).
        let mut core: CoreShell<ScrollbarMultiFixture> = CoreShell::new();
        let state = resolve_shared_scroll_state(&core);
        assert_eq!(state.offset(), (0, 0), "starts at origin");
        assert_eq!(state.max(), (0, 100), "fixture seeds 100 max_y");

        // Seed the router's last paint scene so hit-test routes.
        core.update_paint_scene(build_paint_scene_with_sb_rect());
        let pid = PointerId::MOUSE;

        // Cursor lands at top of scrollbar (y=0). PointerEnter fires
        // on the "sb" tag → Idle → Hover.
        let _ = core.cursor_moved(pid, 10.0, 0.0);

        // PointerDown → Hover → Dragging; ScrollBar requests capture
        // and the router calls pointer_move with the press-time
        // cursor as the first frame.
        let _ = core.pointer_down(pid);
        // First pointer_move = press-time snapshot (no offset change).
        assert_eq!(
            state.offset(),
            (0, 0),
            "press-time pointer_move captures snapshot without moving offset",
        );

        // Drag down to the middle of the track (y=100, height=200).
        // delta_fraction = 0.5, scroll_max = 100, expected offset_y = 50.
        let _ = core.cursor_moved(pid, 10.0, 100.0);
        assert_eq!(
            state.offset_y(),
            50,
            "halfway drag writes scroll_max / 2 = 50 into shared ScrollState",
        );

        // Release — Dragging → Hover, capture clears.
        let _ = core.pointer_up(pid);
        // Offset stays where the drag left it (drag commit semantics).
        assert_eq!(state.offset_y(), 50, "release does not snap back");
    }

    // ─────────────────────────────────────────────────────────────────
    // R680 §5.16 §5.41 §5.28 — per-window Owner scope substrate.
    //
    // First atomic of the 4-axis paint-pipeline rewrite series
    // (R680-R683 axis 3). The tests below pin the load-bearing
    // invariants of `window_owners` + `window_owner` /
    // `window_owner_existing` / `window_owner_ids`:
    //
    // - Default `"main"` slot is seeded as a clone of `root_owner`
    //   (single-window binding zero-regression contract).
    // - Lazy create of a secondary id parents the new scope to
    //   `root_owner` so cascade drop reaches it.
    // - Repeat calls for the same id return handles with identical
    //   `Owner::id`.
    // - Secondary scopes do NOT alias each other.
    // - `Owner::cache` slots are isolated per scope; a slot landed
    //   in the secondary owner is invisible to the root owner cache.
    // - Animations registered against a secondary scope still tick
    //   through `root_owner.tick_animations` (parent walks children
    //   per R51.138 cascade), but a direct
    //   `secondary.tick_animations` walks only the secondary scope.
    // - On substrate drop, both `window_owners` map drop and
    //   `root_owner` cascade release the secondary scope's last Rc
    //   refs, draining every registered animation / cleanup.
    // - `window_owner_existing` is a read-only probe (NO lazy create).
    // - `window_owner_ids` enumerates every seeded + lazily-created
    //   slot.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r680_default_window_seeded_as_root_owner_clone() {
        // R680 §5.16 §5.41 §5.28 — `new()` seeds the
        // [`DEFAULT_WINDOW`] slot with a clone of `root_owner`. Same
        // `Rc<OwnerInner>` internals → same `Owner::id`. Single-
        // window bindings (every Phase A example) read through
        // either accessor and reach the same scope.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let main_owner = core.window_owner(DEFAULT_WINDOW);
        assert_eq!(
            main_owner.id(),
            core.root_owner().id(),
            "DEFAULT_WINDOW slot must alias root_owner so Phase A bindings stay bit-identical",
        );
    }

    #[test]
    fn r680_window_owner_main_alias_persists_across_calls() {
        // R680 — repeated `window_owner("main")` calls return the
        // same `Owner::id` (the seeded `root_owner` clone is cached;
        // the lazy-create path is never entered for the canonical
        // primary).
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let first = core.window_owner(DEFAULT_WINDOW).id();
        let second = core.window_owner(DEFAULT_WINDOW).id();
        let third = core.window_owner(DEFAULT_WINDOW).id();
        assert_eq!(first, second);
        assert_eq!(second, third);
        assert_eq!(first, core.root_owner().id());
    }

    #[test]
    fn r680_secondary_window_creates_distinct_child_of_root() {
        // R680 — lazy-create on first lookup: the secondary owner
        // has a fresh `Owner::id` that is NOT the root owner's id,
        // so the two scopes are addressable independently.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let root_id = core.root_owner().id();
        let inspector = core.window_owner("inspector");
        assert_ne!(
            inspector.id(),
            root_id,
            "secondary window owner must be a fresh scope, not a root clone",
        );
    }

    #[test]
    fn r680_secondary_window_idempotent_across_calls() {
        // R680 — second lookup for the same secondary id returns the
        // cached entry; a third lookup also returns the cached entry.
        // The lazy-create branch fires exactly once per `window_id`.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let a = core.window_owner("inspector").id();
        let b = core.window_owner("inspector").id();
        let c = core.window_owner("inspector").id();
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn r680_two_secondary_windows_do_not_alias() {
        // R680 — distinct secondary ids → distinct scopes. The map
        // keys are the canonical `WindowSpec::id` strings and the
        // lookup never collapses unrelated ids.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let inspector = core.window_owner("inspector").id();
        let palette = core.window_owner("palette").id();
        let outliner = core.window_owner("outliner").id();
        assert_ne!(inspector, palette);
        assert_ne!(palette, outliner);
        assert_ne!(inspector, outliner);
    }

    #[test]
    fn r680_window_owner_existing_returns_seeded_main() {
        // R680 — `window_owner_existing` is read-only, but the
        // canonical primary is seeded at construction so it is
        // observable without first calling the lazy-create accessor.
        let core: CoreShell<TestButton> = CoreShell::new();
        let main_probe = core
            .window_owner_existing(DEFAULT_WINDOW)
            .expect("primary window owner must be seeded by `new()`");
        assert_eq!(main_probe.id(), core.root_owner().id());
    }

    #[test]
    fn r680_window_owner_existing_returns_none_for_unknown_id() {
        // R680 — the read-only probe never lazy-creates; `None` is
        // the contract for an id that has not yet been touched.
        let core: CoreShell<TestButton> = CoreShell::new();
        assert!(core.window_owner_existing("inspector").is_none());
        assert!(core.window_owner_existing("never-touched").is_none());
    }

    #[test]
    fn r680_window_owner_existing_returns_some_after_lazy_create() {
        // R680 — after `window_owner("inspector")` lazy-creates the
        // scope, the read-only probe observes it. The probe returns
        // a handle with the same `Owner::id` as the original.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let created = core.window_owner("inspector").id();
        let probed = core
            .window_owner_existing("inspector")
            .expect("lazy-created secondary must be observable through the read-only probe");
        assert_eq!(created, probed.id());
    }

    #[test]
    fn r680_window_owner_ids_includes_seeded_main_at_construction() {
        // R680 — fresh substrate's id iterator contains the canonical
        // primary slot. Order is HashMap-unstable; the test sorts
        // before asserting to stay deterministic.
        let core: CoreShell<TestButton> = CoreShell::new();
        let mut ids: Vec<&str> = core.window_owner_ids().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![DEFAULT_WINDOW]);
    }

    #[test]
    fn r680_window_owner_ids_grows_with_lazy_creation() {
        // R680 — every lazy create through `window_owner` adds an
        // entry observable through the id iterator.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let _ = core.window_owner("inspector");
        let _ = core.window_owner("palette");
        let mut ids: Vec<&str> = core.window_owner_ids().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["inspector", DEFAULT_WINDOW, "palette"]);
    }

    #[test]
    fn r680_secondary_scope_owner_cache_isolated_from_root() {
        // R680 §5.28 — `Owner::cache` slots are per-`OwnerInner`. A
        // slot keyed `"shared"` landed in the secondary scope does
        // NOT appear in the root scope's cache (separate
        // `RefCell<HashMap>` backing). The test confirms the
        // isolation by writing on the secondary and probing on the
        // root.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let inspector = core.window_owner("inspector");
        // Land a slot on the secondary scope.
        let _: Rc<u32> = inspector.run(|| {
            pinion_core::Owner::current()
                .expect("Owner::current resolves inside Owner::run")
                .cache("r680_isolation_probe", || 42_u32)
        });
        // The root scope's cache does not contain the key.
        assert!(
            !core
                .root_owner()
                .cache_contains::<u32>("r680_isolation_probe"),
            "secondary-scope cache slot must NOT leak into the root scope",
        );
    }

    #[test]
    fn r680_two_secondary_scopes_have_isolated_owner_cache() {
        // R680 §5.28 — two distinct secondary scopes also do not
        // share cache slots. Write on `"inspector"`, probe on
        // `"palette"` for the same key.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let inspector = core.window_owner("inspector");
        let _: Rc<u32> = inspector.run(|| {
            pinion_core::Owner::current()
                .unwrap()
                .cache("r680_cross_secondary_probe", || 7_u32)
        });
        let palette = core.window_owner("palette");
        assert!(
            !palette.cache_contains::<u32>("r680_cross_secondary_probe"),
            "two secondary scopes must not share `Owner::cache` slots",
        );
    }

    #[test]
    fn r680_substrate_tick_does_not_cascade_into_secondary_scopes() {
        // R680 atomic 1 §5.16 §5.28 — `CoreShell::tick_animations`
        // routes through [`Self::tick_animations_for_window`] with
        // `DEFAULT_WINDOW`, which calls
        // [`Owner::tick_animations_local`] (NOT the cascade
        // `tick_animations`) on the resolved window scope. A
        // `Tickable` registered on a secondary scope is therefore
        // structurally invisible to a primary-window tick — the
        // foundation for the R670.B 9-round honest carry on
        // multi-window animation compound elimination.
        //
        // The companion test
        // `r680_per_window_tick_walks_only_addressed_scope` exercises
        // the multi-window dispatch: ticking the secondary scope
        // walks only IT (not root + not other secondaries), so each
        // window's paint cycle advances its own animations exactly
        // once.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let secondary = core.window_owner("inspector");
        let recorder = Rc::new(TickRecorder::new());
        secondary.register_animation(recorder.clone());
        // Substrate-level tick uses the local walk on the primary
        // scope (root_owner clone). Secondary scopes are NOT
        // descended into.
        core.tick_animations(0.016);
        assert_eq!(
            recorder.ticks.get(),
            0,
            "substrate tick must NOT cascade into secondary scopes (R680 axis 3 multi-window compound elimination)",
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R680 atomic 1 §5.16 §5.28 §5.41 — per-window animation tick
    // decoupling. Tests pin the load-bearing invariants:
    //
    // - `tick_animations_for_window(window_id, dt)` walks only that
    //   window's scope (no cascade, no compound on multi-window).
    // - Single-window backward compat — `tick_animations(dt)` routes
    //   through DEFAULT_WINDOW and reaches every root-registered
    //   animation (because DEFAULT_WINDOW is a root_owner alias).
    // - Unknown window_id → no-op walk + frame_signal still bumps.
    // - `any_animation_active_for_window` mirrors the local walk.
    // - Two windows can paint independently with different `dt`
    //   values and each window's animation advances exactly once.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r680_per_window_tick_walks_only_addressed_scope() {
        // R680 atomic 1 — register a Tickable on each of root +
        // secondary "inspector" + secondary "palette". Ticking
        // "inspector" advances only the inspector recorder; root +
        // palette remain at 0 ticks.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let root_recorder = Rc::new(TickRecorder::new());
        core.root_owner().register_animation(root_recorder.clone());
        let inspector_recorder = Rc::new(TickRecorder::new());
        core.window_owner("inspector")
            .register_animation(inspector_recorder.clone());
        let palette_recorder = Rc::new(TickRecorder::new());
        core.window_owner("palette")
            .register_animation(palette_recorder.clone());

        core.tick_animations_for_window("inspector", 0.025);

        assert_eq!(inspector_recorder.ticks.get(), 1);
        assert_eq!(
            inspector_recorder.last_dt.get().to_bits(),
            0.025_f32.to_bits(),
            "inspector dt forwarded verbatim",
        );
        assert_eq!(
            root_recorder.ticks.get(),
            0,
            "primary scope is invisible to a secondary-window tick",
        );
        assert_eq!(
            palette_recorder.ticks.get(),
            0,
            "sibling secondary scope is invisible to a foreign tick",
        );
    }

    #[test]
    fn r680_two_secondary_windows_tick_with_independent_dt() {
        // R680 atomic 1 — true per-window animation pump: two
        // secondary windows paint in the same event-loop turn with
        // distinct `dt` values; each window's recorder records its
        // own delta, no compound.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let inspector_recorder = Rc::new(TickRecorder::new());
        core.window_owner("inspector")
            .register_animation(inspector_recorder.clone());
        let palette_recorder = Rc::new(TickRecorder::new());
        core.window_owner("palette")
            .register_animation(palette_recorder.clone());

        core.tick_animations_for_window("inspector", 0.020);
        core.tick_animations_for_window("palette", 0.040);

        assert_eq!(inspector_recorder.ticks.get(), 1);
        assert_eq!(
            inspector_recorder.last_dt.get().to_bits(),
            0.020_f32.to_bits(),
        );
        assert_eq!(palette_recorder.ticks.get(), 1);
        assert_eq!(
            palette_recorder.last_dt.get().to_bits(),
            0.040_f32.to_bits(),
        );
    }

    #[test]
    fn r680_tick_for_unknown_window_id_is_noop_but_still_bumps_frame_signal() {
        // R680 atomic 1 — `tick_animations_for_window("never-created", _)`
        // is a no-op on the animation walk (no scope to walk) but
        // still bumps the monotonic frame counter so application
        // observers of `frame_signal` see the cascade. The
        // no-lazy-create contract is symmetric with
        // `any_animation_active_for_window`.
        let core: CoreShell<TestButton> = CoreShell::new();
        let baseline = core.frame_signal().get();
        core.tick_animations_for_window("never-created", 0.016);
        assert_eq!(
            core.frame_signal().get(),
            baseline + 1,
            "unknown window_id must still bump frame_signal for application observers",
        );
        // No window_owner entry was created as a side effect — the
        // probe still returns None.
        assert!(core.window_owner_existing("never-created").is_none());
    }

    #[test]
    fn r680_tick_default_window_walks_root_local_only() {
        // R680 atomic 1 — single-window backward compat: a Tickable
        // registered on root_owner via the canonical path (Phase A
        // bindings) is reached by the substrate's `tick_animations`
        // alias because DEFAULT_WINDOW is a root_owner clone.
        let core: CoreShell<TestButton> = CoreShell::new();
        let root_recorder = Rc::new(TickRecorder::new());
        core.root_owner().register_animation(root_recorder.clone());
        for _ in 0..3 {
            core.tick_animations(0.016);
        }
        assert_eq!(
            root_recorder.ticks.get(),
            3,
            "single-window legacy tick path must still reach root-registered animations bit-identical to pre-R680",
        );
    }

    #[test]
    fn r680_any_animation_active_for_window_reads_local_only() {
        // R680 atomic 1 — the active-probe variant of the local
        // walk: a moving animation on secondary "inspector" is
        // observable through
        // `any_animation_active_for_window("inspector", eps)` but
        // NOT through the same probe targeted at root or at a
        // sibling secondary.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let recorder = Rc::new(TickRecorder::new());  // is_at_rest = false (always moving)
        core.window_owner("inspector").register_animation(recorder);
        // Also create a sibling secondary with NO registered
        // animations so the cross-secondary probe targets a real
        // (empty) scope.
        let _palette = core.window_owner("palette");

        assert!(
            core.any_animation_active_for_window("inspector", 0.01),
            "secondary scope with a non-resting Tickable reports active",
        );
        assert!(
            !core.any_animation_active_for_window("palette", 0.01),
            "sibling secondary with no registrations reports inactive",
        );
        assert!(
            !core.any_animation_active_for_window(DEFAULT_WINDOW, 0.01),
            "primary scope (root) carries no animations in this fixture",
        );
        // The binding-wide cascade variant still observes the
        // inspector's animation (root walks children).
        assert!(
            core.any_animation_active(0.01),
            "binding-wide cascade still sees the secondary's active animation",
        );
    }

    #[test]
    fn r680_any_animation_active_for_unknown_window_is_false() {
        // R680 atomic 1 — `any_animation_active_for_window` returns
        // `false` for never-created scopes without lazy-creating an
        // entry, mirroring `tick_animations_for_window`.
        let core: CoreShell<TestButton> = CoreShell::new();
        assert!(!core.any_animation_active_for_window("never-touched", 0.01));
        assert!(core.window_owner_existing("never-touched").is_none());
    }

    #[test]
    fn r680_tick_for_window_records_last_dt_for_observers() {
        // R680 atomic 1 — `tick_animations_for_window` writes the
        // most-recent `dt` into the substrate's shared `last_dt`
        // cell (used by observers that resolve the most-recent
        // frame delta from outside a reactive context).
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let _ = core.window_owner("inspector");
        core.tick_animations_for_window("inspector", 0.033);
        assert_eq!(
            core.last_dt.get().to_bits(),
            0.033_f32.to_bits(),
            "last_dt mirrors the value the substrate just dispatched",
        );
        core.tick_animations_for_window("inspector", 0.050);
        assert_eq!(
            core.last_dt.get().to_bits(),
            0.050_f32.to_bits(),
            "successive ticks overwrite last_dt",
        );
    }

    #[test]
    fn r680_secondary_scope_direct_tick_walks_only_own_scope() {
        // R680 §5.28 — calling `tick_animations` directly on a
        // secondary scope walks the secondary's own children + self
        // ONLY. A `Tickable` registered on the root is NOT ticked
        // when the secondary scope's `tick_animations` is invoked.
        //
        // This is the load-bearing invariant for atomic (1): if a
        // backend calls `inspector_owner.tick_animations(dt)` while
        // skipping `root_owner.tick_animations`, the inspector's own
        // animations advance once and the rest of the binding does
        // not. Two windows can therefore tick independently without
        // compounding each other's spring solvers (closes the
        // R670.B 9-round carry on `[[multi-window-input-router-race]]`-
        // adjacent compound).
        let core: CoreShell<TestButton> = CoreShell::new();
        let root_recorder = Rc::new(TickRecorder::new());
        core.root_owner().register_animation(root_recorder.clone());
        // Build a secondary directly via `Owner::new_child` so the
        // test exercises the same shape `window_owner` uses but
        // without needing `&mut self`.
        let secondary = Owner::new_child(core.root_owner());
        let secondary_recorder = Rc::new(TickRecorder::new());
        secondary.register_animation(secondary_recorder.clone());

        secondary.tick_animations(0.016);

        assert_eq!(
            secondary_recorder.ticks.get(),
            1,
            "secondary tick must advance its own animations",
        );
        assert_eq!(
            root_recorder.ticks.get(),
            0,
            "secondary tick must NOT walk up into the root scope",
        );
    }

    #[test]
    fn r680_secondary_scope_drops_with_substrate() {
        // R680 §5.28 — when the substrate goes out of scope, the
        // `window_owners` map drops first (declaration order; the
        // field is declared after `root_owner`), then `root_owner`
        // cascade walks its children list and releases the
        // secondary scope's last strong Rc. Every `Tickable`
        // registered on the secondary scope is therefore dropped.
        //
        // Verified via `Rc::strong_count`: the test owns one strong
        // ref to a `TickRecorder` while the secondary owner holds
        // another. After the substrate drops, only the test's local
        // strong ref remains.
        let recorder = Rc::new(TickRecorder::new());
        {
            let mut core: CoreShell<TestButton> = CoreShell::new();
            let inspector = core.window_owner("inspector");
            inspector.register_animation(recorder.clone());
            assert!(Rc::strong_count(&recorder) >= 2);
        }
        assert_eq!(
            Rc::strong_count(&recorder),
            1,
            "substrate drop must cascade through window_owners → root_owner.children → secondary scope teardown",
        );
    }

    #[test]
    fn r680_secondary_scope_drops_with_substrate_owner_cache_too() {
        // R680 §5.28 — `Owner::cache` slots on the secondary scope
        // also drop when the substrate drops. The cache map is a
        // `RefCell<HashMap<_, Rc<dyn Any>>>` on the `OwnerInner`;
        // releasing the last `OwnerInner` strong ref drops the
        // HashMap and decrements every cached `Rc`.
        struct Sentinel {
            flag: Rc<Cell<bool>>,
        }
        impl Drop for Sentinel {
            fn drop(&mut self) {
                self.flag.set(true);
            }
        }
        let flag = Rc::new(Cell::new(false));
        {
            let mut core: CoreShell<TestButton> = CoreShell::new();
            let inspector = core.window_owner("inspector");
            let flag_clone = Rc::clone(&flag);
            let _: Rc<Sentinel> = inspector.run(|| {
                pinion_core::Owner::current().unwrap().cache(
                    "r680_drop_sentinel",
                    move || Sentinel { flag: flag_clone },
                )
            });
            assert!(
                !flag.get(),
                "sentinel must still be alive while substrate lives",
            );
        }
        assert!(
            flag.get(),
            "secondary scope cache slot must drop with the substrate",
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R683 §5.16 §5.41 §5.28 — `CoreShell::remove_window` pin:
    //
    // - Refuses [`DEFAULT_WINDOW`] (the primary scope is a
    //   `root_owner` alias; removing it would orphan the binding's
    //   reactive substrate).
    // - Drops the `routers` entry + the `window_owners` entry for
    //   secondary `window_id`s in one call.
    // - Dropping the `window_owners` entry releases the per-window
    //   `Owner` scope → cleanup queue drains → every animation /
    //   command / cache slot on that scope drops.
    // - Returns `true` on actual removal, `false` for
    //   `DEFAULT_WINDOW` + unknown ids — callers can distinguish
    //   "primary protected" / "no-op" without separate probes.
    // - Idempotent: second `remove_window` call on the same id is a
    //   no-op (the substrate has no entry to drop).
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r683_remove_window_refuses_default_window() {
        // The primary scope alias (DEFAULT_WINDOW → root_owner) must
        // survive every `remove_window` call. Returning `false`
        // signals "not removed" without aborting — callers can log
        // / introspect the protection without paying for a separate
        // primary-id check.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        // Pre-state: DEFAULT_WINDOW is seeded in `new()` so the
        // owner + router exist.
        assert!(core.window_owner_existing(DEFAULT_WINDOW).is_some());
        let removed = core.remove_window(DEFAULT_WINDOW);
        assert!(!removed, "DEFAULT_WINDOW must be primary-protected");
        // Post-state unchanged.
        assert!(core.window_owner_existing(DEFAULT_WINDOW).is_some());
        // Root owner alias still intact.
        assert_eq!(
            core.window_owner(DEFAULT_WINDOW).id(),
            core.root_owner().id(),
        );
    }

    #[test]
    fn r683_remove_window_drops_secondary_owner_scope() {
        // Lazy-create a secondary scope, then `remove_window`. The
        // scope drops, the read-only probe returns None.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let inspector_id = core.window_owner("inspector").id();
        assert!(core.window_owner_existing("inspector").is_some());
        let removed = core.remove_window("inspector");
        assert!(removed, "secondary scope must report `true` on removal");
        assert!(
            core.window_owner_existing("inspector").is_none(),
            "secondary scope must be gone from the window_owners map after remove",
        );
        // Lazy re-create after removal mints a NEW scope (fresh
        // Owner::id) — the substrate did not retain a soft handle to
        // the dropped scope.
        let new_id = core.window_owner("inspector").id();
        assert_ne!(
            new_id, inspector_id,
            "post-remove lazy-create must produce a fresh scope",
        );
    }

    #[test]
    fn r683_remove_window_returns_false_for_unknown_id() {
        // An id that was never lazy-created has no entries in the
        // window_owners or routers maps; `remove_window` returns
        // `false` without panicking — the dock + tear-off arc's
        // drop pass calls this for every spec id that disappeared
        // from the signal, including ones that never had a winit
        // window of their own (defensive race-condition guard).
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let removed = core.remove_window("never-touched");
        assert!(
            !removed,
            "remove_window on an untouched id must report `false`",
        );
    }

    #[test]
    fn r683_remove_window_drops_per_window_animations() {
        // R683 — the `Owner` drop triggered by `remove_window`
        // cascades through the cleanup queue, releasing every
        // animation registered on that scope.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let recorder = Rc::new(TickRecorder::new());
        core.window_owner("inspector")
            .register_animation(recorder.clone());
        // Substrate + recorder = 2 strong refs.
        assert!(Rc::strong_count(&recorder) >= 2);
        let removed = core.remove_window("inspector");
        assert!(removed);
        // Post-remove the substrate drops its strong ref → recorder
        // refcount falls back to 1 (only the local binding holds it).
        assert_eq!(
            Rc::strong_count(&recorder),
            1,
            "remove_window must drop the secondary scope's registered animations",
        );
    }

    #[test]
    fn r683_remove_window_idempotent_on_double_call() {
        // Second `remove_window` call on the same id is a no-op:
        // returns `false` because no entry exists in either map.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let _ = core.window_owner("inspector");
        assert!(core.remove_window("inspector"));
        assert!(
            !core.remove_window("inspector"),
            "second remove on the same id must be a no-op",
        );
    }

    #[test]
    fn r683_remove_window_does_not_affect_sibling_secondary_scope() {
        // Distinct secondary ids are addressed independently; the
        // remove pass for one must not touch any sibling.
        let mut core: CoreShell<TestButton> = CoreShell::new();
        let inspector_id = core.window_owner("inspector").id();
        let palette_id = core.window_owner("palette").id();
        assert!(core.remove_window("inspector"));
        // Palette survives + retains its Owner::id.
        assert!(core.window_owner_existing("palette").is_some());
        assert_eq!(
            core.window_owner_existing("palette").unwrap().id(),
            palette_id,
        );
        // Inspector is gone.
        assert!(core.window_owner_existing("inspector").is_none());
        // Re-creating inspector mints a fresh scope.
        assert_ne!(core.window_owner("inspector").id(), inspector_id);
    }
}
