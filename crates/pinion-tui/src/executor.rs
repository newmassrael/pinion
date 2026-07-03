//! R51.160 §5.23 — TUI sibling of `pinion_shell::executor`.
//!
//! Concrete [`Executor`] + [`IntentSink`] impls binding the substrate
//! [`CommandExecutor`](pinion_runtime::CommandExecutor) (R51.156–R51.158)
//! to the TUI shell's tokio runtime + crossterm-driven event loop
//! wake mechanism.
//!
//! # Why this duplicates `pinion-shell::executor` (R51.159)
//!
//! Each backend owns its async runtime binding so neither pulls the
//! other's transitive deps (winit / wgpu / `accesskit_winit` for
//! Vello vs ratatui / crossterm for TUI). The [`TokioExecutor`] impl is
//! byte-identical to the Vello sibling; the [`MpscIntentSink`] is the
//! TUI-specific wake surface — `crossterm` does not expose a
//! winit-style `EventLoopProxy`, so resolved [`Intent`]s travel
//! through a [`std::sync::mpsc::Sender`] and the
//! [`crate::shell::run`] loop drains the receiver between every
//! crossterm `poll` tick (the same cadence the §5.28 R33 animation
//! pump already runs at, so no extra latency on the intent arrival
//! path).
//!
//! A future DRY refactor (e.g. lifting `TokioExecutor` to a
//! `pinion-async` crate behind a Cargo feature) is the textbook step
//! once a third backend lands; at two backends, duplication is the
//! Rule-of-Three threshold — wait for the third consumer.

use std::sync::{Arc, mpsc};

use pinion_core::Intent;
use pinion_runtime::{BoxFuture, CommandTaskHandle, Executor, IntentSink};
use tokio::runtime::Runtime;

/// R51.160 §5.23 — tokio multi-thread [`Executor`] impl for the TUI
/// shell. Sibling of `pinion_shell::TokioExecutor`.
///
/// Builds a private `tokio::runtime::Runtime` at construction time
/// (single worker thread, all features enabled) and dispatches every
/// [`BoxFuture`] through `Runtime::spawn`.
/// The returned [`tokio::task::AbortHandle`] backs the
/// [`CommandTaskHandle`]'s cancel callback so R51.158's per-scope
/// cancellation actually aborts the future at its next `.await` point.
///
/// ## Drop semantics
///
/// `tokio::runtime::Runtime::drop` is blocking — it waits for the
/// worker thread to wind down. For the TUI shell that runs from
/// `main` to terminal teardown, this is the right behaviour: pending
/// commands either complete or get aborted before the process exits.
pub struct TokioExecutor {
    runtime: Runtime,
}

impl TokioExecutor {
    /// Build a tokio multi-thread runtime (1 worker thread,
    /// `enable_all`) and wrap it as the concrete [`Executor`] impl.
    ///
    /// # Errors
    /// Returns the underlying [`std::io::Error`] if the runtime fails
    /// to spin up its worker thread.
    pub fn new() -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("pinion-cmd-tui")
            .build()?;
        Ok(Self { runtime })
    }
}

impl Executor for TokioExecutor {
    fn spawn(&self, future: BoxFuture) -> CommandTaskHandle {
        let join_handle = self.runtime.spawn(future);
        let abort = join_handle.abort_handle();
        CommandTaskHandle::new(move || abort.abort())
    }
}

impl core::fmt::Debug for TokioExecutor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TokioExecutor").finish_non_exhaustive()
    }
}

/// R51.160 §5.23 — `mpsc::Sender<Intent>`-backed [`IntentSink`] for
/// the TUI shell.
///
/// `crossterm` has no winit-style `EventLoopProxy::send_event`
/// equivalent — the only way to nudge the TUI event loop from a
/// worker thread is to write into a channel the shell loop drains.
/// This sink forwards each resolved [`Intent`] into the supplied
/// [`mpsc::Sender`]; the matching [`mpsc::Receiver`] stays with the
/// shell's `run` loop, which calls `try_recv` between every
/// `crossterm::event::poll` tick (the same place the §5.28 R33
/// animation pump runs).
///
/// ## Error absorption
///
/// `mpsc::Sender::send` returns `Result<(), SendError>`; the sink
/// trait is infallible. The closed channel is the only failure mode
/// here (the shell loop dropped the receiver mid-flight), at which
/// point the application is shutting down — the [`Intent`] cannot
/// reach a re-dispatch destination anyway, so dropping it on the
/// floor matches the Vello sibling's `EventLoopProxy::send_event`
/// error absorption.
///
/// ## Why `Sender` (not `SyncSender`)?
///
/// `std::sync::mpsc::Sender` is unbounded and lock-free on the send
/// side; the TUI shell's intent arrival rate is bounded by handler
/// completion rates (typically << 1000/s), so backpressure is a
/// non-issue and the bounded `SyncSender::send` would only add a
/// blocking edge under no realistic workload.
pub struct MpscIntentSink {
    sender: mpsc::Sender<Intent>,
}

impl MpscIntentSink {
    /// Wrap the supplied [`mpsc::Sender`]. The matching
    /// [`mpsc::Receiver`] stays with the shell's `run` loop.
    #[must_use]
    pub fn new(sender: mpsc::Sender<Intent>) -> Self {
        Self { sender }
    }
}

impl IntentSink for MpscIntentSink {
    fn send(&self, intent: Intent) {
        // Closed receiver = app shutting down; drop the intent.
        let _ = self.sender.send(intent);
    }
}

impl core::fmt::Debug for MpscIntentSink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MpscIntentSink").finish_non_exhaustive()
    }
}

/// R51.160 §5.23 — triple returned by [`build_executor_and_sink`].
///
/// `(executor, sink, intent_receiver)`:
///
/// - `executor` — the [`TokioExecutor`] wrapped as `Arc<dyn Executor>`
///   so [`CommandExecutor::new`](pinion_runtime::CommandExecutor::new)
///   can consume it without naming the concrete type.
/// - `sink` — the [`MpscIntentSink`] wrapped as `Arc<dyn IntentSink>`
///   (same erased-trait carrier).
/// - `intent_receiver` — the matching [`mpsc::Receiver`] the shell
///   loop drains via `try_recv` on every poll tick.
pub type ExecutorSinkBundle = (
    Arc<dyn Executor>,
    Arc<dyn IntentSink>,
    mpsc::Receiver<Intent>,
);

/// R51.160 §5.23 — convenience constructor that builds the
/// `(executor, sink, intent_receiver)` [`ExecutorSinkBundle`] for the
/// TUI shell's [`crate::shell::run_with_handlers`] boot path. The
/// receiver stays with the shell loop for `try_recv` drain on every
/// poll tick.
///
/// # Errors
/// Forwards [`TokioExecutor::new`] failure.
pub fn build_executor_and_sink() -> std::io::Result<ExecutorSinkBundle> {
    let (tx, rx) = mpsc::channel::<Intent>();
    let executor: Arc<dyn Executor> = Arc::new(TokioExecutor::new()?);
    let sink: Arc<dyn IntentSink> = Arc::new(MpscIntentSink::new(tx));
    Ok((executor, sink, rx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::future::Future;
    use core::pin::Pin;
    use core::sync::atomic::{AtomicBool, Ordering};
    use pinion_core::external::IntrospectValue;

    type LocalBoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

    fn poll_until<F: Fn() -> bool>(check: F, ms: u64) {
        let start = std::time::Instant::now();
        while !check() && start.elapsed() < std::time::Duration::from_millis(ms) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn tokio_executor_builds_runtime() {
        let executor = TokioExecutor::new().expect("runtime build succeeds");
        let dbg = format!("{executor:?}");
        assert!(dbg.contains("TokioExecutor"));
    }

    #[test]
    fn tokio_executor_drives_future_to_completion() {
        let executor = TokioExecutor::new().unwrap();
        let observed = Arc::new(AtomicBool::new(false));
        let observed_clone = Arc::clone(&observed);
        let future: LocalBoxFuture = Box::pin(async move {
            observed_clone.store(true, Ordering::SeqCst);
        });
        let _handle = executor.spawn(future);
        poll_until(|| observed.load(Ordering::SeqCst), 1000);
        assert!(observed.load(Ordering::SeqCst));
    }

    #[test]
    fn tokio_executor_cancel_aborts_pending_future() {
        let executor = TokioExecutor::new().unwrap();
        let observed = Arc::new(AtomicBool::new(false));
        let observed_clone = Arc::clone(&observed);
        let future: LocalBoxFuture = Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            observed_clone.store(true, Ordering::SeqCst);
        });
        let handle = executor.spawn(future);
        std::thread::sleep(std::time::Duration::from_millis(50));
        handle.cancel();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!observed.load(Ordering::SeqCst));
        assert!(handle.is_cancelled());
    }

    #[test]
    fn mpsc_intent_sink_forwards_one_intent() {
        let (tx, rx) = mpsc::channel::<Intent>();
        let sink = MpscIntentSink::new(tx);
        sink.send(Intent::new_static("test.evt", IntrospectValue::Int(42)));
        let received = rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .expect("intent should arrive");
        assert_eq!(received.tag_str(), "test.evt");
        assert_eq!(received.payload, IntrospectValue::Int(42));
    }

    #[test]
    fn mpsc_intent_sink_preserves_send_order() {
        let (tx, rx) = mpsc::channel::<Intent>();
        let sink = MpscIntentSink::new(tx);
        sink.send(Intent::new_static("a", IntrospectValue::Int(1)));
        sink.send(Intent::new_static("b", IntrospectValue::Int(2)));
        sink.send(Intent::new_static("c", IntrospectValue::Int(3)));
        let tags: Vec<String> = (0..3)
            .map(|_| {
                rx.recv_timeout(std::time::Duration::from_millis(100))
                    .unwrap()
            })
            .map(|i| i.tag_str().to_string())
            .collect();
        assert_eq!(tags, vec!["a", "b", "c"]);
    }

    #[test]
    fn mpsc_intent_sink_closed_receiver_absorbs_error() {
        let (tx, rx) = mpsc::channel::<Intent>();
        let sink = MpscIntentSink::new(tx);
        drop(rx); // close the channel
        // send must NOT panic even though the receiver is gone.
        sink.send(Intent::new_static("orphan", IntrospectValue::Null));
    }

    #[test]
    fn build_executor_and_sink_returns_triple() {
        let (_exec, _sink, _rx) = build_executor_and_sink().expect("tokio runtime + channel build");
    }
}
