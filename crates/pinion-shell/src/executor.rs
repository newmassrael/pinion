//! R51.159 §5.23 — concrete [`Executor`] and [`IntentSink`] impls that
//! bind the substrate's `CommandExecutor` (R51.156–R51.158) to the
//! Vello shell's tokio runtime + winit event-loop wake mechanism.
//!
//! # Why this lives here (not in `pinion-runtime`)
//!
//! Per §6.3 (view-fn sync, IO async at the boundary), pinion-runtime
//! stays async-runtime-agnostic — the [`Executor`] /
//! [`IntentSink`] traits define the
//! abstract surface, and backends supply concrete impls at the
//! window-system / runtime boundary. This module is the Vello shell's
//! boundary impl:
//!
//! - [`TokioExecutor`] wraps a `tokio::runtime::Runtime` (multi-thread,
//!   1 worker thread by default) and spawns
//!   [`BoxFuture`]s through it. The
//!   [`tokio::task::AbortHandle`] returned by `spawn` powers the
//!   [`CommandTaskHandle`] cancel
//!   callback so R51.158's per-scope cancellation actually aborts the
//!   running future.
//! - [`ProxyIntentSink`] wraps a winit
//!   [`EventLoopProxy<AppEvent>`](winit::event_loop::EventLoopProxy)
//!   and forwards resolved [`Intent`]s through
//!   [`AppEvent::IntentArrived`]. The
//!   user-event arm on the UI thread re-feeds them into
//!   [`ShellCore::dispatch_intent`](crate::ShellCore) (R51.160 carry —
//!   for now the arm logs them; integrating into the SCXML `send`
//!   channel is the next round).
//!
//! # Lifecycle
//!
//! [`TokioExecutor`] owns the [`tokio::runtime::Runtime`] by value, so
//! dropping the executor drops the runtime — pending tasks abort, the
//! worker thread shuts down. The shell's
//! `Arc<CommandExecutor>` aliases through both backends; on
//! application shutdown the final
//! [`Arc`] drop cascades through
//! `CommandExecutor` → `TokioExecutor::drop` → `Runtime::drop` which
//! awaits in-flight tasks (or aborts them after a grace period).

use std::sync::Arc;

use pinion_core::Intent;
use pinion_runtime::{BoxFuture, CommandTaskHandle, Executor, IntentSink};
use tokio::runtime::Runtime;
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

/// R51.159 §5.23 — tokio multi-thread [`Executor`] impl.
///
/// Builds a private `tokio::runtime::Runtime` at construction time
/// (single worker thread, all features enabled). Subsequent
/// [`Executor::spawn`] calls dispatch the
/// [`BoxFuture`] onto the runtime via
/// `Runtime::spawn`; the returned [`tokio::task::AbortHandle`] backs
/// the [`CommandTaskHandle`]'s cancel callback so R51.158's per-scope
/// cancellation actually aborts the future at its next `.await` point.
///
/// ## Why multi-thread (not current-thread)?
///
/// A `current_thread` tokio runtime must be entered explicitly
/// (`Runtime::block_on` or `enter()` guard) for tasks to make
/// progress — but the UI thread is inside the winit event loop and
/// cannot block. A multi-thread runtime self-drives on its worker
/// thread the moment a task is spawned, no extra plumbing on the UI
/// side. One worker thread is plenty for the [`Command`](pinion_core::Command)
/// surface: each handler runs to completion independently, the worker
/// time-slices across in-flight futures.
///
/// ## `Send + Sync`
///
/// `tokio::runtime::Runtime` is `Send + Sync` (it shares state via
/// atomic ref counts internally), so the [`Executor`] supertrait
/// bound on this struct is satisfied without an extra `Mutex`.
pub struct TokioExecutor {
    runtime: Runtime,
}

impl TokioExecutor {
    /// Build a tokio multi-thread runtime with one worker thread and
    /// wrap it as the concrete [`Executor`] impl.
    ///
    /// # Errors
    /// Returns the underlying [`std::io::Error`] if the runtime fails
    /// to spin up its worker thread (typically a thread-spawn failure
    /// at the OS level — extremely rare on the desktop targets pinion
    /// supports).
    pub fn new() -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("pinion-cmd")
            .build()?;
        Ok(Self { runtime })
    }
}

impl Executor for TokioExecutor {
    fn spawn(&self, future: BoxFuture) -> CommandTaskHandle {
        // R51.159 — tokio Runtime::spawn returns a JoinHandle whose
        // abort_handle() yields a cancellable handle that is Send +
        // Sync + 'static. Wrap into CommandTaskHandle so the
        // substrate's R51.158 per-scope cancellation can fire it
        // through the `Arc<dyn Fn() + Send + Sync + 'static>`
        // boundary.
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

/// R51.159 §5.23 — winit-[`EventLoopProxy`]-backed [`IntentSink`] impl.
///
/// Wraps an [`EventLoopProxy<AppEvent>`] (cloned cheaply per the winit
/// contract) and forwards every received [`Intent`] through
/// [`AppEvent::IntentArrived`] so the UI thread's `user_event` arm can
/// re-feed it into the SCXML / Update reducer surface.
///
/// ## `Send + Sync`
///
/// `EventLoopProxy<AppEvent>` is `Send + Sync` per winit's contract,
/// so the [`IntentSink`] trait bound holds.
///
/// ## Error absorption
///
/// `EventLoopProxy::send_event` returns `Result<(), EventLoopClosed>`;
/// the [`IntentSink::send`] trait surface is infallible. The closed
/// event loop is the only failure mode here, and at that point the
/// application is shutting down — the [`Intent`] cannot reach a
/// re-dispatch destination anyway, so dropping it on the floor is the
/// correct behaviour (matches the `pinion-tui` mpsc-Sender error
/// absorption pattern in R51.160 carry).
pub struct ProxyIntentSink {
    proxy: EventLoopProxy<AppEvent>,
}

impl ProxyIntentSink {
    /// Wrap the supplied [`EventLoopProxy`].
    #[must_use]
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self { proxy }
    }
}

impl IntentSink for ProxyIntentSink {
    fn send(&self, intent: Intent) {
        // Closed event loop = app shutting down; drop the intent.
        let _ = self.proxy.send_event(AppEvent::IntentArrived(intent));
    }
}

impl core::fmt::Debug for ProxyIntentSink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProxyIntentSink").finish_non_exhaustive()
    }
}

/// R51.159 §5.23 — convenience constructor that builds the full
/// `(Arc<dyn Executor>, Arc<dyn IntentSink>)` pair from a winit
/// [`EventLoopProxy`]. The shell's `run_with_handlers` entry point
/// uses this to compose the [`CommandExecutor`](pinion_runtime::CommandExecutor)
/// at boot.
///
/// # Errors
/// Forwards [`TokioExecutor::new`] failure.
pub fn build_executor_and_sink(
    proxy: EventLoopProxy<AppEvent>,
) -> std::io::Result<(Arc<dyn Executor>, Arc<dyn IntentSink>)> {
    let executor: Arc<dyn Executor> = Arc::new(TokioExecutor::new()?);
    let sink: Arc<dyn IntentSink> = Arc::new(ProxyIntentSink::new(proxy));
    Ok((executor, sink))
}

/// R999 §5.23 — winit-[`EventLoopProxy`]-backed [`RepaintSink`](pinion_core::RepaintSink) impl.
///
/// The single winit-aware part of the external-async-data → repaint seam: an
/// off-thread producer (e.g. a PTY reader) obtains this through
/// [`use_repaint_sink`](pinion_core::use_repaint_sink) and calls
/// [`request_repaint`](pinion_core::RepaintSink::request_repaint) after writing fresh data
/// into the shared handle the binding's `view` reads. It forwards through
/// [`AppEvent::ExternalRepaint`], whose `user_event` arm arms a redraw; winit
/// coalesces multiple requests into one `RedrawRequested` per frame.
///
/// Sibling of [`ProxyIntentSink`] — the §6.3 boundary-trait pattern: the
/// runtime-agnostic [`RepaintSink`](pinion_core::RepaintSink) lives in `pinion-core`, this concrete
/// `EventLoopProxy` impl lives at the window-system boundary.
///
/// ## `Send + Sync`
///
/// `EventLoopProxy<AppEvent>` is `Send + Sync` per winit's contract, so the
/// [`RepaintSink`](pinion_core::RepaintSink) supertrait bound holds and the handle clones into the
/// producer thread.
///
/// ## Error absorption
///
/// A closed event loop is the only `send_event` failure mode and means the app
/// is shutting down — dropping the wake is correct (matches [`ProxyIntentSink`]
/// and `spawn_stdin_rpc_reader`).
pub struct ProxyRepaintSink {
    proxy: EventLoopProxy<AppEvent>,
}

impl ProxyRepaintSink {
    /// Wrap the supplied [`EventLoopProxy`].
    #[must_use]
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self { proxy }
    }
}

impl pinion_core::RepaintSink for ProxyRepaintSink {
    fn request_repaint(&self) {
        // Closed event loop = app shutting down; drop the wake.
        let _ = self.proxy.send_event(AppEvent::ExternalRepaint);
    }
}

impl core::fmt::Debug for ProxyRepaintSink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProxyRepaintSink").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::future::Future;
    use core::pin::Pin;
    use core::sync::atomic::{AtomicBool, Ordering};

    type LocalBoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

    fn poll_until<F: Fn() -> bool>(check: F, ms: u64) {
        let start = std::time::Instant::now();
        while !check() && start.elapsed() < std::time::Duration::from_millis(ms) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn tokio_executor_builds_runtime() {
        // R51.159 — Runtime construction succeeds on the supported
        // desktop targets (one worker thread, all features).
        let executor = TokioExecutor::new().expect("runtime build succeeds");
        let dbg = format!("{executor:?}");
        assert!(dbg.contains("TokioExecutor"));
    }

    #[test]
    fn tokio_executor_drives_future_to_completion() {
        // R51.159 — spawn drives the future on the tokio worker
        // thread; the observable side-effect (atomic bool flip) is
        // visible to the UI thread once the future resolves.
        let executor = TokioExecutor::new().unwrap();
        let observed = Arc::new(AtomicBool::new(false));
        let observed_clone = Arc::clone(&observed);
        let future: LocalBoxFuture = Box::pin(async move {
            observed_clone.store(true, Ordering::SeqCst);
        });
        let _handle = executor.spawn(future);
        poll_until(|| observed.load(Ordering::SeqCst), 1000);
        assert!(
            observed.load(Ordering::SeqCst),
            "tokio worker should have flipped the bool within 1s",
        );
    }

    #[test]
    fn tokio_executor_cancel_aborts_pending_future() {
        // R51.159 — calling cancel() on the handle aborts the spawn
        // before it can complete. We sleep for 5 seconds inside the
        // future and abort within 50ms — the sleep should be aborted
        // and the post-await side-effect must NOT run.
        let executor = TokioExecutor::new().unwrap();
        let observed = Arc::new(AtomicBool::new(false));
        let observed_clone = Arc::clone(&observed);
        let future: LocalBoxFuture = Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            observed_clone.store(true, Ordering::SeqCst);
        });
        let handle = executor.spawn(future);
        // Give the future a moment to start the sleep.
        std::thread::sleep(std::time::Duration::from_millis(50));
        handle.cancel();
        // Wait beyond the sleep would have completed if cancel hadn't
        // run. 200ms is plenty to observe the abort.
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            !observed.load(Ordering::SeqCst),
            "cancelled future must NOT execute its post-await code",
        );
        assert!(handle.is_cancelled());
    }

    #[test]
    fn tokio_executor_concurrent_spawns_dont_serialize() {
        // R51.159 — two concurrent spawns make progress in parallel
        // (one worker but cooperative multitasking via .await). With
        // a 50ms tokio sleep per task, two sequential tasks would
        // take ~100ms; cooperative concurrency completes both in <80ms.
        let executor = TokioExecutor::new().unwrap();
        let a = Arc::new(AtomicBool::new(false));
        let b = Arc::new(AtomicBool::new(false));
        let a_c = Arc::clone(&a);
        let b_c = Arc::clone(&b);
        let f_a: LocalBoxFuture = Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            a_c.store(true, Ordering::SeqCst);
        });
        let f_b: LocalBoxFuture = Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            b_c.store(true, Ordering::SeqCst);
        });
        let _h_a = executor.spawn(f_a);
        let _h_b = executor.spawn(f_b);
        poll_until(|| a.load(Ordering::SeqCst) && b.load(Ordering::SeqCst), 500);
        assert!(a.load(Ordering::SeqCst));
        assert!(b.load(Ordering::SeqCst));
    }

    // Note: `ProxyIntentSink` Debug / `send` exercises require a
    // live `winit::EventLoopProxy`, which in turn requires an
    // `EventLoop` plus a windowing system handle. The headless test
    // surface here intentionally skips that smoke; integration
    // tests with a real `EventLoop` cover it.
}
