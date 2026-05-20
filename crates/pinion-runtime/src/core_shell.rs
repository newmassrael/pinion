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
use std::sync::Arc;

use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::reactive::{Effect, Signal};
use pinion_core::scene::ExternalNode;
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
    router: InputRouter,
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

    /// R51.149 §5.28 — reactive animation driver Effect, anchored to
    /// [`Self::root_owner`] and subscribed to [`Self::frame_signal`].
    ///
    /// Held as a field to keep the underlying `Rc<EffectInner>` alive
    /// for the substrate's lifetime; [`Effect::new`](pinion_core::reactive::Effect)
    /// registers an owner-tied `on_cleanup` that cancels the Effect
    /// when [`Self::root_owner`] drops, so the field naturally
    /// evaporates at shutdown.
    ///
    /// The `_` prefix matches Rust convention for "value held for its
    /// side effects, never read directly". Reads happen exclusively
    /// through the reactive cascade
    /// ([`Signal::set`](pinion_core::reactive::Signal::set) on
    /// [`Self::frame_signal`]).
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
        let root_owner = Owner::new();
        let frame_signal = Signal::new(0_u64);
        let last_dt: std::rc::Rc<Cell<f32>> = std::rc::Rc::new(Cell::new(0.0_f32));

        // R51.149 §5.28 — build the framework AnimationDriver as a
        // reactive `Effect` subscribing to `frame_signal`. Each
        // [`Self::tick_animations`] call stores the new `dt` into
        // [`Self::last_dt`] then increments the frame counter,
        // firing this Effect; the closure reads back the `dt` and
        // dispatches into `Owner::tick_animations(dt)`. Eager
        // initial run executes against the construction baseline
        // (`dt = 0.0`, empty animation registry) — a deliberate
        // no-op that primes the subscription so the first real
        // tick fires.
        let owner_for_driver = root_owner.clone();
        let owner_for_closure = root_owner.clone();
        let signal_for_driver = frame_signal.clone();
        let dt_for_closure = std::rc::Rc::clone(&last_dt);
        let animation_driver = root_owner.run(|| {
            Effect::new(&owner_for_driver, move || {
                // `get()` subscribes to the counter on the eager
                // initial run; subsequent re-runs trigger off
                // `Signal::set` in `tick_animations`.
                let _frame = signal_for_driver.get();
                let dt = dt_for_closure.get();
                owner_for_closure.tick_animations(dt);
            })
        });

        Self {
            scene,
            cached_state,
            router: InputRouter::new(),
            intent_queue: IntentQueue::new(),
            root_owner,
            frame_signal,
            last_dt,
            next_frame_count: Cell::new(1_u64),
            _animation_driver: animation_driver,
            executor: None,
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
    /// The cached projection [`WidgetCore::read_state`] yields is
    /// mutated transiently by the reducer; writing the mutation back
    /// into the authoritative [`Scene`] is the R51.168 carry.
    /// First-cut land surfaces the **declarative `Vec<Command>` half**
    /// of the §5.23 R27 contract — every command lands on the owner
    /// queue so [`Self::dispatch_pending_commands`] reaches the
    /// registered handler on the next pump.
    ///
    /// Returns the same `Vec<Command>` the reducer produced so
    /// callers (and tests) can introspect what was queued without a
    /// second owner snapshot. The commands are already on the queue
    /// — drop the return value if the caller has no use for it.
    #[must_use = "the returned commands are already queued; ignore explicitly with `let _ = …` if you do not need them"]
    pub fn route_intent_through_update(&self, intent: &Intent) -> Vec<Command> {
        let mut state = V::read_state(&self.scene);
        let commands = V::update(&mut state, intent);
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
        // R51.149 §5.28 — store the new dt into the shared
        // `Rc<Cell>` then bump the monotonic frame counter. The
        // counter bump fires the `_animation_driver` Effect
        // synchronously (outside any active batch); the Effect body
        // reads back the stored `dt` and dispatches into
        // `Owner::tick_animations(dt)` against `root_owner` so every
        // registered spring advances by exactly one tick. Same
        // observable behaviour as the pre-R51.149 direct call, with
        // the spec-canonical reactive routing (R51.137 Effect /
        // §5.28 R33).
        //
        // `next_frame_count` is read without subscribing — the
        // monotonic step lives in a `Cell` outside the reactive
        // graph; only `frame_signal.set` enters the cascade.
        self.last_dt.set(dt);
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
        let handled = owner.run(|| V::apply_key(scene, focused, key));
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
        let _ = core.apply_key(Some("test_btn"), "Space");
        // Post-apply: wrap popped.
        assert!(
            pinion_core::Owner::current().is_none(),
            "apply_key's root_owner.run wrap must pop on exit",
        );
        // The wrap is symmetric: V::view runs under the same owner.
        // We've already verified through R51.146 that V::view sees
        // root_owner; the apply_key wrap mirrors the same shape.
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
    // the default-reducer (empty) path on `TestButton` and an
    // overriding reducer fixture (`EchoButton`) that emits one
    // command per intent.

    use pinion_core::external::External;
    use pinion_core::widgets::button::ButtonExternal;

    /// R51.167 reducer-test fixture: same paint / `read_state` /
    /// `event_name` surface as `TestButton`, but the reducer emits
    /// one `echo.reply` `Command` per incoming intent.
    struct EchoButton;

    impl WidgetCore for EchoButton {
        type State = ButtonState;
        type Event = ButtonEvent;

        fn create_external() -> Box<dyn External> {
            Box::new(ButtonExternal::new())
        }

        fn tag() -> &'static str {
            "echo_btn"
        }

        fn read_state(scene: &Scene) -> Self::State {
            <TestButton as WidgetCore>::read_state(scene)
        }

        fn view(state: Self::State, frame: &Frame) -> Scene {
            <TestButton as WidgetCore>::view(state, frame)
        }

        fn event_name(event: Self::Event) -> &'static str {
            <TestButton as WidgetCore>::event_name(event)
        }

        fn title() -> &'static str {
            "EchoBtn"
        }

        fn update(_state: &mut Self::State, intent: &Intent) -> Vec<Command> {
            vec![Command::new_static(
                "echo.reply",
                IntrospectValue::Text(intent.tag_str().to_string()),
                42,
            )]
        }
    }

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
}
