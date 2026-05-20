//! [`Executor`] + [`CommandTaskHandle`] + [`CommandExecutor`] composite.
//!
//! Closes the §5.23 R27 dispatch pipeline by composing the three pieces
//! that landed in earlier rounds:
//!
//! - [`HandlerRegistry`](super::registry::HandlerRegistry) (R51.141) —
//!   the `kind → Handler` lookup table.
//! - [`Executor`] (this module) — the substrate-side async runtime
//!   abstraction; concrete impls live at the backend boundary
//!   (`pinion-shell` / `pinion-tui` / `pinion-rpc`) per §6.3 (view-fn
//!   sync, IO async at the boundary).
//! - [`IntentSink`](super::sink::IntentSink) (R51.156) — the wake
//!   surface the resolved [`Intent`] travels through back to the UI
//!   thread.
//!
//! The composite [`CommandExecutor`] dispatches one [`Command`] by:
//!
//! 1. Looking up the handler via `registry.dispatch(cmd)` (lazy —
//!    returns `None` when no handler is registered for the `kind`).
//! 2. Wrapping the returned [`HandlerFuture`] (`Output = Intent`) into
//!    a `Output = ()` boxed future that, on resolution, calls
//!    `sink.send(intent)`.
//! 3. Spawning the wrapped future via `executor.spawn(...)`.
//! 4. Returning the [`CommandTaskHandle`] the executor handed back so
//!    R51.158's per-scope cancellation map can abort prior in-flight
//!    work when a new [`Command`] arrives on the same scope.
//!
//! ## What lands at R51.156 (this round)
//!
//! - [`Executor`] trait + [`CommandTaskHandle`] cancel-handle.
//! - [`CommandExecutor`] composite, returns `Option<CommandTaskHandle>`
//!   from [`CommandExecutor::dispatch`].
//! - [`BlockOnExecutor`] — synchronous `futures_executor::block_on`
//!   reference impl. Drives the wrapped future to completion inside
//!   `spawn` itself; useful for tests and for binaries that prefer
//!   sync dispatch (an embedded controller polling one Command at a
//!   time, or a CLI driver running through `pinion-rpc` without an
//!   async runtime).
//!
//! ## What carries
//!
//! - **R51.157** — `CoreShell::dispatch_pending_commands` walks
//!   `Owner::take_pending_commands_recursive` and feeds each
//!   [`Command`] to the [`CommandExecutor`].
//! - **R51.158** — per-scope cancellation: [`CommandExecutor`] gains a
//!   `Mutex<BTreeMap<scope_id, CommandTaskHandle>>` that aborts the
//!   prior in-flight handle when a new command arrives on the same
//!   scope (R27 Solid pattern).
//! - **R51.159** — `pinion-shell` concrete tokio current-thread
//!   [`Executor`] + winit [`EventLoopProxy`]-based [`IntentSink`].
//! - **carry** — `tokio` / `async-executor` / custom impls;
//!   [`Executor`] stays sufficient because it erases the concrete
//!   future at the spawn boundary.

use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use pinion_core::Command;

use super::handler::HandlerFuture;
use super::registry::HandlerRegistry;
use super::sink::IntentSink;

/// Boxed fire-and-forget future the [`Executor`] surface speaks.
///
/// `Pin<Box<dyn Future<Output = ()> + Send + 'static>>`:
///
/// - `Output = ()` because the [`CommandExecutor`] wraps the original
///   [`HandlerFuture`] (`Output = Intent`) into a closure that consumes
///   the [`Intent`] by handing it to the [`IntentSink`] — the executor
///   never sees the raw [`Intent`] value.
/// - `Send` so a multi-thread runtime (tokio, async-executor, custom
///   thread pool) can poll the future on any worker.
/// - `'static` so the executor stores the future as an owned value
///   independent of the spawn-site stack frame.
pub type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Async runtime abstraction the [`CommandExecutor`] hands futures to.
///
/// `Send + Sync + 'static` because the composite [`CommandExecutor`]
/// is itself shared across the substrate (UI thread) and the spawned
/// futures (worker threads) through `Arc<dyn Executor>`.
///
/// ## Concrete impls
///
/// - [`BlockOnExecutor`] (this module) — synchronous reference;
///   drives the future to completion inside `spawn`.
/// - `pinion-shell` (R51.159) — tokio current-thread runtime.
/// - `pinion-tui` (carry) — tokio current-thread runtime sharing the
///   shell's runtime when both backends compile together.
///
/// ## Cancellation contract
///
/// `spawn` returns a [`CommandTaskHandle`]; calling
/// [`CommandTaskHandle::cancel`] requests the executor abort the
/// future. The exact semantics ("future stops at the next await point"
/// / "callback runs to completion regardless" / "no-op") are
/// impl-defined. R51.158's per-scope cancellation map relies on the
/// handle's [`CommandTaskHandle::cancel`] being callable from any
/// thread.
pub trait Executor: Send + Sync + 'static {
    /// Spawn a fire-and-forget [`BoxFuture`]. Returns a
    /// [`CommandTaskHandle`] that callers (specifically the
    /// [`CommandExecutor`]) can use to abort the spawned task.
    fn spawn(&self, future: BoxFuture) -> CommandTaskHandle;
}

/// Cancellable handle returned by [`Executor::spawn`].
///
/// Holds an `Arc`-counted cancel callback plus an idempotent
/// [`AtomicBool`] guard so [`Self::cancel`] is safe to call repeatedly
/// (the second call is a no-op).
///
/// ## Drop semantics
///
/// Drop does NOT auto-cancel. This matches `tokio::task::JoinHandle`'s
/// detach-on-drop behaviour and keeps per-scope cancellation
/// observable: R51.158's [`CommandExecutor`] stores the handle in a
/// per-scope `BTreeMap` and explicitly calls `cancel()` when a new
/// [`Command`] arrives on the same scope. If Drop auto-cancelled, the
/// map insertion itself would race-cancel the just-spawned task.
///
/// ## Thread safety
///
/// `Send + Sync` so the executor can hand the handle to any consumer.
/// `cancel()` takes `&self` so multiple aliases (e.g. a copy in the
/// per-scope map plus one returned to the dispatch site) all see the
/// same cancelled state.
pub struct CommandTaskHandle {
    cancel: Arc<dyn Fn() + Send + Sync + 'static>,
    cancelled: Arc<AtomicBool>,
}

impl CommandTaskHandle {
    /// Construct a handle from a cancel callback. The callback runs at
    /// most once: subsequent [`Self::cancel`] calls short-circuit on
    /// the [`AtomicBool`] guard.
    pub fn new(cancel: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            cancel: Arc::new(cancel),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Construct a handle whose [`Self::cancel`] is a no-op. The
    /// [`BlockOnExecutor`] returns this because the future is already
    /// complete by the time `spawn` returns — there is nothing to
    /// cancel.
    #[must_use]
    pub fn no_op() -> Self {
        Self::new(|| {})
    }

    /// Request cancellation. Idempotent: only the first call invokes
    /// the underlying callback.
    pub fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            (self.cancel)();
        }
    }

    /// `true` once [`Self::cancel`] has been called at least once.
    /// Observers polling the flag see the AcqRel-ordered write.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl core::fmt::Debug for CommandTaskHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CommandTaskHandle")
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl Clone for CommandTaskHandle {
    /// Clones share the same cancel callback + cancelled flag. Calling
    /// `cancel()` on either clone marks both as cancelled.
    fn clone(&self) -> Self {
        Self {
            cancel: Arc::clone(&self.cancel),
            cancelled: Arc::clone(&self.cancelled),
        }
    }
}

/// §5.23 dispatch composite — [`HandlerRegistry`] + [`Executor`] +
/// [`IntentSink`] bundled into one struct.
///
/// Owns the registry by value (the boot-time registration phase
/// completes before [`CommandExecutor::new`] runs); the executor and
/// sink are `Arc<dyn ...>` so the substrate's `Arc<CommandExecutor>`
/// can be cloned freely. The composite itself is shared through
/// `Arc<CommandExecutor>` by backends that drain pending commands
/// from multiple sites (per-frame paint, per-RPC dispatch).
///
/// ## Lifecycle
///
/// ```ignore
/// // Boot: build the registry once.
/// let mut registry = HandlerRegistry::new();
/// registry.register("http.get", Arc::new(my_http_handler));
/// registry.register("clipboard.write", Arc::new(my_clipboard_handler));
///
/// // Wrap with executor + sink, share as Arc.
/// let exec: Arc<dyn Executor> = Arc::new(BlockOnExecutor);
/// let sink: Arc<dyn IntentSink> = Arc::new(VecSink::new());
/// let cmd_exec = Arc::new(CommandExecutor::new(registry, exec, sink));
///
/// // Per-frame: drain pending commands.
/// for cmd in owner.take_pending_commands_recursive() {
///     let _ = cmd_exec.dispatch(cmd);
/// }
/// ```
///
/// `Send + Sync` because every field is — `Arc<dyn Executor>` and
/// `Arc<dyn IntentSink>` carry the trait bound, [`HandlerRegistry`]
/// is `Send + Sync` (its `BTreeMap` keys are `Cow<'static, str>` and
/// values are `Arc<dyn Handler>` which is `Send + Sync` per the
/// [`Handler`](super::handler::Handler) supertrait bound).
pub struct CommandExecutor {
    registry: HandlerRegistry,
    executor: Arc<dyn Executor>,
    sink: Arc<dyn IntentSink>,
    /// R51.158 §5.23 — per-scope in-flight task tracker.
    ///
    /// Keyed by [`Command::scope_id`](pinion_core::Command::scope_id);
    /// the value carries the [`Command`] dispatched on that scope plus
    /// the [`CommandTaskHandle`] [`Executor::spawn`] returned. Kept
    /// alive so:
    ///
    /// - A follow-up [`Command`] on the same scope can abort the
    ///   prior task before spawning the next one (the §5.23 R27 Solid
    ///   pattern: "new Command from same scope cancels prior
    ///   in-flight").
    /// - R51.162 §5.23 — the `scene/commands` RPC method can surface
    ///   the kind + payload of every in-flight task to AI agents for
    ///   inspection via [`Self::in_flight_snapshot`]. Pre-R51.162 the
    ///   value was just the handle; storing the [`Command`] alongside
    ///   pays one [`Command::clone`] per dispatch in exchange for full
    ///   introspection symmetry with the pending-side
    ///   [`Owner::pending_commands_recursive`](pinion_core::reactive::Owner::pending_commands_recursive)
    ///   shape.
    ///
    /// Entries are inserted on every successful [`Self::dispatch`] and
    /// removed (with a `cancel()` call) when:
    ///
    /// - A new [`Command`] arrives on the same `scope_id`
    ///   (auto-abort on dispatch).
    /// - The substrate calls [`Self::cancel_scope`] explicitly (e.g.
    ///   the [`Owner`](pinion_core::Owner) hosting the scope is about
    ///   to drop and the backend wants to surface the cancellation
    ///   before the entry's `scope_id` becomes unreachable).
    ///
    /// Entries left behind by natural completion stay in the map until
    /// the next dispatch on the same scope cancels them (no-op for
    /// completed tasks: the executor's cancel callback is invoked but
    /// the underlying future has already resolved). This is a textbook
    /// trade — auto-removing on natural completion would require the
    /// wrapped future to capture an [`Arc<Mutex<BTreeMap<...>>>`] back
    /// into the executor, and the resulting drop ordering between the
    /// sink delivery and the map cleanup is fragile across async
    /// runtimes. Bounded growth: at most one entry per unique scope
    /// id, and scope ids are
    /// [`Owner::id`](pinion_core::reactive::Owner::id)s which collide
    /// only after `u64` wraparound (i.e. never).
    ///
    /// [`Mutex`] (not `RefCell`) so the trait bound `Send + Sync` on
    /// [`CommandExecutor`] is preserved (the trait bounds on
    /// [`Executor`] and [`IntentSink`] force it; `Arc<CommandExecutor>`
    /// is the production carrier shape).
    in_flight: Mutex<BTreeMap<u64, InFlightEntry>>,
}

/// R51.162 §5.23 — value stored in [`CommandExecutor::in_flight`].
///
/// Holds the [`Command`] dispatched (for introspection via
/// [`CommandExecutor::in_flight_snapshot`]) plus the
/// [`CommandTaskHandle`] returned by the executor (for cancellation).
/// `pub(crate)` because callers reach the entry only through
/// the [`CommandExecutor`] accessors — no need to expose the layout
/// at the crate surface.
pub(crate) struct InFlightEntry {
    pub(crate) command: Command,
    pub(crate) handle: CommandTaskHandle,
}

impl CommandExecutor {
    /// Build a new dispatch composite.
    ///
    /// `registry` is moved in by value; downstream callers obtain
    /// post-construction read access via [`Self::registry`] (mut access
    /// is intentionally absent at R51.156 — swappable registration is
    /// R27 carry, R51.158+).
    #[must_use]
    pub fn new(
        registry: HandlerRegistry,
        executor: Arc<dyn Executor>,
        sink: Arc<dyn IntentSink>,
    ) -> Self {
        Self {
            registry,
            executor,
            sink,
            in_flight: Mutex::new(BTreeMap::new()),
        }
    }

    /// Read-only borrow of the underlying [`HandlerRegistry`]. Tests
    /// inspect registered kinds; backends introspect for the
    /// `scene/commands` RPC method (carry).
    #[must_use]
    pub fn registry(&self) -> &HandlerRegistry {
        &self.registry
    }

    /// Dispatch one [`Command`] — look up the handler, wrap the
    /// resulting [`HandlerFuture`] so the resolved [`Intent`] reaches
    /// the [`IntentSink`], and spawn via [`Executor::spawn`].
    ///
    /// Returns `None` when the registry has no handler for
    /// [`Command::kind_str`](pinion_core::Command::kind_str) — callers
    /// decide whether to log, drop, or surface the unhandled command.
    /// The R51.157 drain pump collects unhandled commands into a Vec
    /// so the backend can stderr-log without mutating the queue.
    /// Unhandled commands do NOT touch the in-flight tracker.
    ///
    /// ## R51.158 — per-scope cancellation
    ///
    /// Before spawning, [`Self::dispatch`] checks
    /// [`Self::in_flight`](Self) for an entry keyed by
    /// [`Command::scope_id`](pinion_core::Command::scope_id). If one
    /// is found, the prior [`CommandTaskHandle`] is removed and
    /// [`CommandTaskHandle::cancel`] is called on it before the new
    /// task spawns. The returned handle is also tracked in
    /// [`Self::in_flight`] so a third dispatch on the same scope
    /// would abort *this* task in turn.
    ///
    /// The handle returned to the caller shares its `cancelled` flag
    /// with the clone stored in [`Self::in_flight`] (the
    /// [`CommandTaskHandle::clone`] impl shares both the cancel
    /// callback and the [`AtomicBool`]). Cancelling either alias
    /// flips the other.
    ///
    /// # Panics
    /// Panics if the internal [`Mutex`] guarding the in-flight map is
    /// poisoned — a panicked worker thread inside another
    /// [`Self::dispatch`] / [`Self::cancel_scope`] / [`Self::in_flight_len`]
    /// call is the only way to reach this state, and that is a
    /// programming error rather than a recoverable runtime condition.
    #[must_use = "the returned CommandTaskHandle is also tracked in the executor's in-flight map; storing it lets the caller observe cancellation set by a follow-up same-scope dispatch"]
    pub fn dispatch(&self, command: Command) -> Option<CommandTaskHandle> {
        let scope = command.scope_id;

        // R51.158 R27 Solid pattern — cancel prior in-flight task on
        // the same scope before spawning the new one. `cancel()` is
        // idempotent and no-op for already-completed tasks (the
        // executor's cancel callback runs at most once per handle).
        if let Some(prior) = self
            .in_flight
            .lock()
            .expect("CommandExecutor::in_flight poisoned")
            .remove(&scope)
        {
            prior.handle.cancel();
        }

        // R51.162 §5.23 — clone the command so the in-flight tracker
        // can surface kind + payload through `in_flight_snapshot`
        // while the original ownership moves into the registry
        // dispatch path (which itself moves it into the future).
        let command_for_tracker = command.clone();
        let future: HandlerFuture = self.registry.dispatch(command)?;
        let sink = Arc::clone(&self.sink);
        let wrapped: BoxFuture = Box::pin(async move {
            let intent = future.await;
            sink.send(intent);
        });
        let handle = self.executor.spawn(wrapped);

        // Track for future cancellation. The clone shares the
        // cancelled flag + cancel callback with the returned handle
        // (R51.156 `CommandTaskHandle::clone` contract), so either
        // alias's `cancel()` flips both.
        self.in_flight
            .lock()
            .expect("CommandExecutor::in_flight poisoned")
            .insert(
                scope,
                InFlightEntry {
                    command: command_for_tracker,
                    handle: handle.clone(),
                },
            );

        Some(handle)
    }

    /// R51.158 §5.23 — cancel any in-flight task tracked for
    /// `scope_id`.
    ///
    /// Returns `Some(handle)` when a tracked handle was removed (and
    /// its `cancel()` called); `None` when no entry was tracked for
    /// the scope. The returned handle is the same clone family as
    /// every other alias of the original — callers that need to
    /// observe `is_cancelled()` after cancellation either keep their
    /// own clone or read it off the returned handle.
    ///
    /// # Panics
    /// Panics if the internal [`Mutex`] guarding the in-flight map is
    /// poisoned.
    pub fn cancel_scope(&self, scope_id: u64) -> Option<CommandTaskHandle> {
        let prior = self
            .in_flight
            .lock()
            .expect("CommandExecutor::in_flight poisoned")
            .remove(&scope_id);
        if let Some(ref entry) = prior {
            entry.handle.cancel();
        }
        prior.map(|entry| entry.handle)
    }

    /// R51.162 §5.23 — non-draining snapshot of every in-flight (or
    /// completed-but-not-yet-superseded) [`Command`] tracked by this
    /// executor.
    ///
    /// Returns the [`Command`]s in `BTreeMap` iteration order
    /// (`scope_id` ascending), deterministic for two snapshots taken
    /// at the same logical instant — matters for AI introspection
    /// where the same query must hash identically. Cloned out of the
    /// tracker so callers can drop the lock immediately and the
    /// underlying map keeps tracking subsequent dispatches.
    ///
    /// Companion to
    /// [`Owner::pending_commands_recursive`](pinion_core::reactive::Owner::pending_commands_recursive)
    /// on the pending side; the §5.23 `scene/commands` RPC method
    /// (R51.161 + R51.162) merges both into the
    /// `{ pending: [...], in_flight: [...] }` wire shape.
    ///
    /// # Panics
    /// Panics if the internal [`Mutex`] is poisoned.
    #[must_use]
    pub fn in_flight_snapshot(&self) -> Vec<Command> {
        self.in_flight
            .lock()
            .expect("CommandExecutor::in_flight poisoned")
            .values()
            .map(|entry| entry.command.clone())
            .collect()
    }

    /// R51.158 §5.23 — number of in-flight (or completed-but-not-yet
    /// -superseded) task handles tracked. Useful for tests and for
    /// the `scene/commands` RPC method (carry).
    ///
    /// # Panics
    /// Panics if the internal [`Mutex`] is poisoned — a panicked
    /// worker thread inside [`Self::dispatch`] / [`Self::cancel_scope`]
    /// is a programming error, not a recoverable runtime condition.
    #[must_use]
    pub fn in_flight_len(&self) -> usize {
        self.in_flight
            .lock()
            .expect("CommandExecutor::in_flight poisoned")
            .len()
    }

    /// R51.158 §5.23 — `true` when `scope_id` has a tracked handle.
    ///
    /// # Panics
    /// Panics if the internal [`Mutex`] is poisoned.
    #[must_use]
    pub fn has_in_flight(&self, scope_id: u64) -> bool {
        self.in_flight
            .lock()
            .expect("CommandExecutor::in_flight poisoned")
            .contains_key(&scope_id)
    }
}

impl core::fmt::Debug for CommandExecutor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CommandExecutor")
            .field("registry_len", &self.registry.len())
            .field("in_flight_len", &self.in_flight_len())
            .finish_non_exhaustive()
    }
}

/// Reference [`Executor`] impl that runs each future to completion
/// inside [`Executor::spawn`] via [`futures_executor::block_on`].
///
/// Useful for:
///
/// - Tests — deterministic single-thread execution, no runtime setup.
/// - CLI / batch drivers that prefer one [`Command`] at a time.
/// - Embedded controllers where pulling a full async runtime is
///   prohibitive.
///
/// Cancellation is a no-op: by the time [`Executor::spawn`] returns,
/// the future has already resolved. R51.158's per-scope cancellation
/// map is a noop with this executor — a new [`Command`] on the same
/// scope just spawns a fresh future after the prior one completed.
///
/// The struct is zero-sized — instantiate as `BlockOnExecutor` (no
/// `::new`). Reference equality between two `Arc<BlockOnExecutor>`
/// instances is irrelevant because each clone of `Arc<dyn Executor>`
/// still calls into the same trait method.
#[derive(Debug, Default, Clone, Copy)]
pub struct BlockOnExecutor;

impl Executor for BlockOnExecutor {
    fn spawn(&self, future: BoxFuture) -> CommandTaskHandle {
        futures_executor::block_on(future);
        CommandTaskHandle::no_op()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::handler::{Handler, HandlerFuture};
    use crate::command::sink::VecSink;
    use pinion_core::external::IntrospectValue;
    use pinion_core::Intent;

    fn echo_handler() -> Arc<dyn Handler> {
        Arc::new(|cmd: Command| -> HandlerFuture {
            Box::pin(async move {
                Intent::new_owned(format!("echo.{}", cmd.kind_str()), cmd.payload)
            })
        })
    }

    fn registry_with(kind: &'static str) -> HandlerRegistry {
        let mut reg = HandlerRegistry::new();
        reg.register(kind, echo_handler());
        reg
    }

    fn build_block_on(registry: HandlerRegistry) -> (CommandExecutor, Arc<VecSink>) {
        let sink = Arc::new(VecSink::new());
        let exec: Arc<dyn Executor> = Arc::new(BlockOnExecutor);
        let sink_dyn: Arc<dyn IntentSink> = sink.clone();
        let cmd_exec = CommandExecutor::new(registry, exec, sink_dyn);
        (cmd_exec, sink)
    }

    // ────────────────────────────────────────────────────────────────
    // R51.156 — CommandTaskHandle
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn handle_cancel_invokes_callback_exactly_once() {
        let observed = Arc::new(AtomicBool::new(false));
        let observed_clone = Arc::clone(&observed);
        let handle = CommandTaskHandle::new(move || {
            observed_clone.store(true, Ordering::SeqCst);
        });
        assert!(!handle.is_cancelled());
        handle.cancel();
        assert!(handle.is_cancelled());
        assert!(observed.load(Ordering::SeqCst));
    }

    #[test]
    fn handle_cancel_idempotent() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_clone = Arc::clone(&calls);
        let handle = CommandTaskHandle::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
        });
        handle.cancel();
        handle.cancel();
        handle.cancel();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "repeated cancel() must invoke the callback exactly once",
        );
    }

    #[test]
    fn handle_no_op_cancel_does_not_panic() {
        let handle = CommandTaskHandle::no_op();
        handle.cancel();
        assert!(handle.is_cancelled());
    }

    #[test]
    fn handle_clones_share_cancelled_flag() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_clone = Arc::clone(&calls);
        let handle = CommandTaskHandle::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
        });
        let alias = handle.clone();
        assert!(!alias.is_cancelled());
        handle.cancel();
        assert!(alias.is_cancelled(), "clones share the cancelled flag");
        alias.cancel();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "shared flag dedups across clones",
        );
    }

    #[test]
    fn handle_debug_format_reports_cancelled_state() {
        let handle = CommandTaskHandle::no_op();
        let before = format!("{handle:?}");
        assert!(before.contains("cancelled: false"));
        handle.cancel();
        let after = format!("{handle:?}");
        assert!(after.contains("cancelled: true"));
    }

    // ────────────────────────────────────────────────────────────────
    // R51.156 — BlockOnExecutor reference impl
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn block_on_executor_drives_future_to_completion() {
        let observed = Arc::new(AtomicBool::new(false));
        let observed_clone = Arc::clone(&observed);
        let future: BoxFuture = Box::pin(async move {
            observed_clone.store(true, Ordering::SeqCst);
        });
        let handle = BlockOnExecutor.spawn(future);
        assert!(observed.load(Ordering::SeqCst));
        assert!(!handle.is_cancelled());
    }

    #[test]
    fn block_on_executor_cancel_handle_is_no_op() {
        let future: BoxFuture = Box::pin(async {});
        let handle = BlockOnExecutor.spawn(future);
        handle.cancel();
        // Idempotent no-op cancel — the future already resolved.
        assert!(handle.is_cancelled());
    }

    // ────────────────────────────────────────────────────────────────
    // R51.156 — CommandExecutor composite
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn dispatch_unknown_kind_returns_none() {
        let (exec, sink) = build_block_on(HandlerRegistry::new());
        let handle = exec.dispatch(Command::new_static(
            "nope",
            IntrospectValue::Null,
            7,
        ));
        assert!(handle.is_none(), "no handler registered → None");
        assert!(sink.is_empty(), "no intent must reach the sink");
    }

    #[test]
    fn dispatch_known_kind_routes_intent_to_sink() {
        let (exec, sink) = build_block_on(registry_with("http.get"));
        let handle = exec.dispatch(Command::new_static(
            "http.get",
            IntrospectValue::Text("/api".into()),
            42,
        ));
        assert!(handle.is_some(), "known handler returns a handle");
        let drained = sink.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].tag_str(), "echo.http.get");
        assert_eq!(drained[0].payload, IntrospectValue::Text("/api".into()));
    }

    #[test]
    fn dispatch_multiple_commands_preserves_order_to_sink() {
        let mut reg = HandlerRegistry::new();
        reg.register("a", echo_handler());
        reg.register("b", echo_handler());
        let (exec, sink) = build_block_on(reg);
        let _h1 = exec.dispatch(Command::new_static("a", IntrospectValue::Int(1), 0));
        let _h2 = exec.dispatch(Command::new_static("b", IntrospectValue::Int(2), 0));
        let _h3 = exec.dispatch(Command::new_static("a", IntrospectValue::Int(3), 0));
        let drained = sink.drain();
        assert_eq!(drained.len(), 3);
        let tags: Vec<&str> = drained.iter().map(Intent::tag_str).collect();
        assert_eq!(tags, vec!["echo.a", "echo.b", "echo.a"]);
        let payloads: Vec<&IntrospectValue> =
            drained.iter().map(|i| &i.payload).collect();
        assert_eq!(
            payloads,
            vec![
                &IntrospectValue::Int(1),
                &IntrospectValue::Int(2),
                &IntrospectValue::Int(3),
            ],
        );
    }

    #[test]
    fn dispatch_returns_no_op_handle_with_block_on_executor() {
        let (exec, _sink) = build_block_on(registry_with("k"));
        let handle = exec
            .dispatch(Command::new_static("k", IntrospectValue::Null, 0))
            .expect("known kind");
        // BlockOnExecutor's handle is a no-op cancel; cancelling it
        // does nothing observable but stays valid.
        assert!(!handle.is_cancelled());
        handle.cancel();
        assert!(handle.is_cancelled());
    }

    #[test]
    fn executor_registry_accessor_exposes_kinds() {
        let mut reg = HandlerRegistry::new();
        reg.register("a", echo_handler());
        reg.register("b", echo_handler());
        let (exec, _sink) = build_block_on(reg);
        let kinds: Vec<&str> = exec.registry().kinds().collect();
        assert_eq!(kinds, vec!["a", "b"]);
        assert_eq!(exec.registry().len(), 2);
    }

    #[test]
    fn executor_debug_reports_registry_size() {
        let mut reg = HandlerRegistry::new();
        reg.register("k1", echo_handler());
        reg.register("k2", echo_handler());
        let (exec, _sink) = build_block_on(reg);
        let dbg = format!("{exec:?}");
        assert!(
            dbg.contains("registry_len: 2"),
            "Debug must report registry_len, got: {dbg}",
        );
    }

    #[test]
    fn dispatch_handler_observing_scope_id_returns_via_intent() {
        // R51.156 — Handler receives the full Command including
        // scope_id; the resulting Intent reflects whatever the
        // handler chose to surface. The substrate doesn't strip
        // scope_id; the handler can serialize it into the payload.
        let mut reg = HandlerRegistry::new();
        reg.register(
            "introspect.scope",
            Arc::new(|cmd: Command| -> HandlerFuture {
                let scope = cmd.scope_id;
                Box::pin(async move {
                    Intent::new_static(
                        "scope_back",
                        IntrospectValue::Int(i64::try_from(scope).unwrap_or(i64::MAX)),
                    )
                })
            }),
        );
        let (exec, sink) = build_block_on(reg);
        let _h = exec.dispatch(Command::new_static(
            "introspect.scope",
            IntrospectValue::Null,
            17,
        ));
        let drained = sink.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].payload, IntrospectValue::Int(17));
    }

    #[test]
    fn dispatch_sink_observes_owned_payload_clones() {
        // R51.156 — the wrap path moves the IntentSink reference into
        // the future and clones the resolved Intent payload through
        // by-value handling. Verify a Text payload reaches the sink
        // intact.
        let (exec, sink) = build_block_on(registry_with("clipboard.write"));
        let _h = exec.dispatch(Command::new_owned(
            "clipboard.write".to_string(),
            IntrospectValue::Text("hello world".to_string()),
            0,
        ));
        let drained = sink.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(
            drained[0].payload,
            IntrospectValue::Text("hello world".to_string()),
        );
    }

    #[test]
    fn dispatch_with_arc_shared_executor_serves_multiple_callers() {
        // R51.156 — the substrate's typical sharing pattern is
        // Arc<CommandExecutor>; multiple sites dispatch through the
        // same composite without re-creating it. Verify each dispatch
        // independently routes through the same sink.
        let (exec, sink) = build_block_on(registry_with("audio.play"));
        let shared = Arc::new(exec);
        for i in 0..4 {
            let alias = Arc::clone(&shared);
            let _h = alias.dispatch(Command::new_static(
                "audio.play",
                IntrospectValue::Int(i64::from(i)),
                0,
            ));
        }
        let drained = sink.drain();
        assert_eq!(drained.len(), 4);
        let payloads: Vec<i64> = drained
            .iter()
            .filter_map(|i| i.payload.as_i64())
            .collect();
        assert_eq!(payloads, vec![0, 1, 2, 3]);
    }

    // ────────────────────────────────────────────────────────────────
    // R51.158 §5.23 — per-scope cancellation (R27 Solid pattern)
    // ────────────────────────────────────────────────────────────────

    /// Test-only [`Executor`] that records every cancel callback fire
    /// into a shared `Vec<usize>` keyed by spawn-order id. Lets tests
    /// verify the executor-side cancel actually ran (the
    /// [`CommandTaskHandle`]'s shared `AtomicBool` flag only tells us
    /// `cancel()` was called; this tells us the executor's own cancel
    /// callback ran).
    #[derive(Default)]
    struct TrackingExecutor {
        next_id: Mutex<usize>,
        cancelled: Arc<Mutex<Vec<usize>>>,
    }

    impl TrackingExecutor {
        fn new() -> (Self, Arc<Mutex<Vec<usize>>>) {
            let log = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    next_id: Mutex::new(0),
                    cancelled: Arc::clone(&log),
                },
                log,
            )
        }
    }

    impl Executor for TrackingExecutor {
        fn spawn(&self, future: BoxFuture) -> CommandTaskHandle {
            let mut id_guard = self.next_id.lock().expect("next_id poisoned");
            *id_guard += 1;
            let id = *id_guard;
            drop(id_guard);
            // Drive the future to completion synchronously (same shape
            // as BlockOnExecutor) so test ordering is deterministic.
            futures_executor::block_on(future);
            let log = Arc::clone(&self.cancelled);
            CommandTaskHandle::new(move || {
                log.lock().expect("cancelled log poisoned").push(id);
            })
        }
    }

    fn build_tracking(
        registry: HandlerRegistry,
    ) -> (CommandExecutor, Arc<VecSink>, Arc<Mutex<Vec<usize>>>) {
        let sink = Arc::new(VecSink::new());
        let (track, cancelled_log) = TrackingExecutor::new();
        let exec: Arc<dyn Executor> = Arc::new(track);
        let sink_dyn: Arc<dyn IntentSink> = sink.clone();
        let cmd_exec = CommandExecutor::new(registry, exec, sink_dyn);
        (cmd_exec, sink, cancelled_log)
    }

    #[test]
    fn r51_158_dispatch_inserts_into_in_flight() {
        let (exec, _sink) = build_block_on(registry_with("k"));
        assert_eq!(exec.in_flight_len(), 0);
        let _h = exec
            .dispatch(Command::new_static("k", IntrospectValue::Null, 7))
            .expect("known kind");
        assert_eq!(exec.in_flight_len(), 1);
        assert!(exec.has_in_flight(7));
        assert!(!exec.has_in_flight(8));
    }

    #[test]
    fn r51_158_dispatch_unknown_kind_does_not_pollute_in_flight() {
        // R51.158 — unknown kinds short-circuit before the in-flight
        // insert; the tracker stays empty.
        let (exec, sink) = build_block_on(HandlerRegistry::new());
        assert!(exec
            .dispatch(Command::new_static("nope", IntrospectValue::Null, 99))
            .is_none());
        assert_eq!(exec.in_flight_len(), 0, "no insert for unhandled kind");
        assert!(sink.is_empty());
    }

    #[test]
    fn r51_158_dispatch_same_scope_cancels_prior_handle() {
        // R51.158 R27 Solid pattern — a second dispatch on the same
        // scope_id auto-cancels the prior tracked handle. The returned
        // handle from the first dispatch reflects cancellation via
        // the shared cancelled flag.
        let (exec, _sink) = build_block_on(registry_with("k"));
        let h1 = exec
            .dispatch(Command::new_static("k", IntrospectValue::Int(1), 42))
            .expect("first dispatch returns a handle");
        assert!(!h1.is_cancelled());
        let h2 = exec
            .dispatch(Command::new_static("k", IntrospectValue::Int(2), 42))
            .expect("second dispatch returns a handle");
        assert!(
            h1.is_cancelled(),
            "first handle must observe cancellation via shared AtomicBool",
        );
        assert!(!h2.is_cancelled(), "second handle is fresh + not cancelled");
        assert_eq!(exec.in_flight_len(), 1, "only the second handle is tracked");
        assert!(exec.has_in_flight(42));
    }

    #[test]
    fn r51_158_dispatch_different_scopes_keep_independent_handles() {
        // R51.158 — different scope ids do not interfere: dispatching
        // on scope B does not cancel scope A.
        let (exec, _sink) = build_block_on(registry_with("k"));
        let h_a = exec
            .dispatch(Command::new_static("k", IntrospectValue::Int(1), 11))
            .expect("scope_a dispatch");
        let h_b = exec
            .dispatch(Command::new_static("k", IntrospectValue::Int(2), 22))
            .expect("scope_b dispatch");
        assert!(!h_a.is_cancelled());
        assert!(!h_b.is_cancelled());
        assert_eq!(exec.in_flight_len(), 2);
        assert!(exec.has_in_flight(11));
        assert!(exec.has_in_flight(22));
    }

    #[test]
    fn r51_158_cancel_scope_returns_and_marks_cancelled() {
        let (exec, _sink) = build_block_on(registry_with("k"));
        let h = exec
            .dispatch(Command::new_static("k", IntrospectValue::Null, 5))
            .expect("dispatch");
        assert!(!h.is_cancelled());
        let returned = exec
            .cancel_scope(5)
            .expect("scope tracked → some returned");
        assert!(returned.is_cancelled());
        assert!(h.is_cancelled(), "shared flag propagates to original handle");
        assert!(!exec.has_in_flight(5), "scope removed after cancel");
        assert_eq!(exec.in_flight_len(), 0);
    }

    #[test]
    fn r51_158_cancel_scope_unknown_returns_none() {
        let (exec, _sink) = build_block_on(registry_with("k"));
        assert!(exec.cancel_scope(99).is_none());
        assert_eq!(exec.in_flight_len(), 0);
    }

    #[test]
    fn r51_158_dispatch_then_cancel_scope_twice_idempotent() {
        // R51.158 — once a scope is cancelled / removed, a second
        // cancel_scope on the same id returns None (no double-fire of
        // the cancel callback).
        let (exec, _sink, log) = build_tracking(registry_with("k"));
        let _h = exec
            .dispatch(Command::new_static("k", IntrospectValue::Null, 3))
            .expect("dispatch");
        let _ = exec.cancel_scope(3);
        let second = exec.cancel_scope(3);
        assert!(second.is_none());
        let cancelled = log.lock().expect("log poisoned").clone();
        assert_eq!(
            cancelled,
            vec![1],
            "executor cancel callback fires exactly once",
        );
    }

    #[test]
    fn r51_158_dispatch_three_times_same_scope_only_latest_tracked() {
        // R51.158 — successive dispatches each cancel the prior; the
        // tracker ends with exactly one entry pointing at the latest.
        let (exec, _sink, log) = build_tracking(registry_with("k"));
        let h1 = exec
            .dispatch(Command::new_static("k", IntrospectValue::Int(1), 7))
            .unwrap();
        let h2 = exec
            .dispatch(Command::new_static("k", IntrospectValue::Int(2), 7))
            .unwrap();
        let h3 = exec
            .dispatch(Command::new_static("k", IntrospectValue::Int(3), 7))
            .unwrap();
        assert!(h1.is_cancelled());
        assert!(h2.is_cancelled());
        assert!(!h3.is_cancelled());
        assert_eq!(exec.in_flight_len(), 1);
        let cancelled = log.lock().expect("log poisoned").clone();
        // h1 cancelled on h2 dispatch; h2 cancelled on h3 dispatch.
        assert_eq!(cancelled, vec![1, 2]);
    }

    #[test]
    fn r51_158_cancel_scope_with_tracking_executor_observes_callback_fire() {
        // R51.158 — verifies the executor-side cancel callback (not
        // just the CommandTaskHandle flag) runs when cancel_scope is
        // invoked. The TrackingExecutor records the spawn id of each
        // cancelled task; we assert the right id appears.
        let (exec, _sink, log) = build_tracking(registry_with("k"));
        let _h = exec
            .dispatch(Command::new_static("k", IntrospectValue::Null, 100))
            .expect("dispatch");
        assert!(log.lock().expect("log poisoned").is_empty());
        let _ = exec.cancel_scope(100);
        let cancelled = log.lock().expect("log poisoned").clone();
        assert_eq!(cancelled, vec![1], "executor cancel ran exactly once");
    }

    #[test]
    fn r51_158_debug_format_reports_in_flight_len() {
        // R51.158 — Debug now surfaces both registry_len and
        // in_flight_len so a single eprintln debugging session
        // reveals the dispatch state at a glance.
        let (exec, _sink) = build_block_on(registry_with("k"));
        let dbg = format!("{exec:?}");
        assert!(
            dbg.contains("in_flight_len: 0"),
            "Debug must report in_flight_len pre-dispatch, got: {dbg}",
        );
        let _h = exec
            .dispatch(Command::new_static("k", IntrospectValue::Null, 1))
            .unwrap();
        let dbg = format!("{exec:?}");
        assert!(
            dbg.contains("in_flight_len: 1"),
            "Debug must report in_flight_len post-dispatch, got: {dbg}",
        );
    }

    // ────────────────────────────────────────────────────────────────
    // R51.162 §5.23 — in_flight_snapshot introspection
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn r51_162_in_flight_snapshot_empty_when_no_dispatches() {
        let (exec, _sink) = build_block_on(registry_with("k"));
        let snap = exec.in_flight_snapshot();
        assert!(snap.is_empty());
    }

    #[test]
    fn r51_162_in_flight_snapshot_surfaces_kind_payload_scope() {
        // R51.162 — every entry exposes the dispatched Command intact:
        // kind + payload + scope_id, ready for the scene/commands wire
        // shape.
        let (exec, _sink) = build_block_on(registry_with("http.get"));
        let _h = exec
            .dispatch(Command::new_static(
                "http.get",
                IntrospectValue::Text("/api/v1".into()),
                42,
            ))
            .unwrap();
        let snap = exec.in_flight_snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].kind_str(), "http.get");
        assert_eq!(snap[0].payload, IntrospectValue::Text("/api/v1".into()));
        assert_eq!(snap[0].scope_id, 42);
    }

    #[test]
    fn r51_162_in_flight_snapshot_deterministic_btreemap_order() {
        // R51.162 — BTreeMap iteration is lexicographic by key
        // (scope_id ascending). Two snapshots taken at the same
        // logical instant must hash identically.
        let mut reg = HandlerRegistry::new();
        reg.register("a", echo_handler());
        reg.register("b", echo_handler());
        let (exec, _sink) = build_block_on(reg);
        // Dispatch in mixed scope-id order; snapshot must reorder.
        let _h_b = exec
            .dispatch(Command::new_static("a", IntrospectValue::Int(2), 50))
            .unwrap();
        let _h_a = exec
            .dispatch(Command::new_static("a", IntrospectValue::Int(1), 20))
            .unwrap();
        let _h_c = exec
            .dispatch(Command::new_static("b", IntrospectValue::Int(3), 80))
            .unwrap();
        let snap = exec.in_flight_snapshot();
        let scope_ids: Vec<u64> = snap.iter().map(|c| c.scope_id).collect();
        assert_eq!(scope_ids, vec![20, 50, 80]);
    }

    #[test]
    fn r51_162_in_flight_snapshot_excludes_cancelled_scope() {
        // R51.162 — cancel_scope removes the entry from the tracker;
        // subsequent snapshot must not include it.
        let (exec, _sink) = build_block_on(registry_with("k"));
        let _h_a = exec
            .dispatch(Command::new_static("k", IntrospectValue::Null, 11))
            .unwrap();
        let _h_b = exec
            .dispatch(Command::new_static("k", IntrospectValue::Null, 22))
            .unwrap();
        assert_eq!(exec.in_flight_snapshot().len(), 2);
        let _ = exec.cancel_scope(11);
        let snap = exec.in_flight_snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].scope_id, 22);
    }

    #[test]
    fn r51_162_in_flight_snapshot_replaces_on_same_scope_dispatch() {
        // R51.162 — same-scope dispatch cancels the prior entry +
        // inserts the new one. Snapshot reflects only the latest.
        let (exec, _sink) = build_block_on(registry_with("k"));
        let _h1 = exec
            .dispatch(Command::new_static("k", IntrospectValue::Int(1), 7))
            .unwrap();
        let _h2 = exec
            .dispatch(Command::new_static("k", IntrospectValue::Int(2), 7))
            .unwrap();
        let snap = exec.in_flight_snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].payload, IntrospectValue::Int(2));
    }

    #[test]
    fn r51_162_in_flight_snapshot_clones_independent_of_tracker() {
        // R51.162 — the returned Vec is owned; mutating it must not
        // affect the underlying tracker (matches the
        // `pending_commands` clone-independence guarantee).
        let (exec, _sink) = build_block_on(registry_with("k"));
        let _h = exec
            .dispatch(Command::new_static("k", IntrospectValue::Null, 1))
            .unwrap();
        let mut snap = exec.in_flight_snapshot();
        snap.clear();
        // Tracker still holds the entry.
        assert_eq!(exec.in_flight_len(), 1);
    }
}
