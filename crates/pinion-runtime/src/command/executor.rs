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
use std::sync::Arc;

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
    ///
    /// On success returns the [`CommandTaskHandle`] the executor
    /// produced — R51.158 stores it in a per-scope `BTreeMap` so a
    /// follow-up [`Command`] on the same `scope_id` can abort the
    /// prior in-flight task before spawning the new one.
    #[must_use = "the returned CommandTaskHandle should be stored so a follow-up Command on the same scope can cancel the prior task"]
    pub fn dispatch(&self, command: Command) -> Option<CommandTaskHandle> {
        let future: HandlerFuture = self.registry.dispatch(command)?;
        let sink = Arc::clone(&self.sink);
        let wrapped: BoxFuture = Box::pin(async move {
            let intent = future.await;
            sink.send(intent);
        });
        Some(self.executor.spawn(wrapped))
    }
}

impl core::fmt::Debug for CommandExecutor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CommandExecutor")
            .field("registry_len", &self.registry.len())
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
}
