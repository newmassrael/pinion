//! §5.7 PR-47 — reusable RPC transport adapters over the winit-free
//! [`pinion_rpc`] injection seam.
//!
//! The shell's built-in transport reads JSON-RPC frames off the process's
//! own stdin and writes responses to stdout, so an RPC endpoint could only
//! exist where the parent process happened to wire fd 0 / fd 1. This crate
//! provides an *execution-independent, always-on* endpoint instead: a Unix
//! domain socket at a fixed path that any producer can reach at any time,
//! driven through the same [`RpcIngress`] seam and hence the same dispatch
//! core.
//!
//! Layering (PR-47 three layers):
//!
//! - **Dispatch core** — owned by pinion, unchanged, transport-agnostic.
//! - **Transport mechanism** — THIS crate: the socket accept loop, framing,
//!   and per-connection response routing. Reusable by any consumer, GUI or
//!   TUI, because it depends on no *backend* — only on the seam above and on
//!   the OS primitives below (`AF_UNIX` and `poll(2)`).
//! - **Transport policy** (when the endpoint is on/off, where it lives) —
//!   owned by the *consumer*: it chooses the path, declares the endpoint's
//!   [`Exposure`] at bind, holds the [`TransportControl`], and toggles
//!   [`TransportControl::set_exposure`] or drops the control to tear the
//!   endpoint down. There is no framework-side toggle mechanism; runtime
//!   on/off falls out of the consumer owning the transport's lifetime.
//!
//! # R1541 — what a caller may assume about cost
//!
//! A **fresh connection costs the same as a request on an established one**.
//! That is a contract, not an implementation detail, because it decides what
//! shape of consumer this endpoint can carry: a per-invocation CLI opens a
//! connection per call and has nothing to amortise a per-connection cost
//! over. The accept loop therefore waits on the listener's readiness rather
//! than on a clock, and the property is pinned by counter — not by a
//! duration — through [`TransportControl::accept_wakeups`].
//!
//! Concretely: an idle endpoint performs **no** syscalls, a connection is
//! admitted as soon as the kernel has one (measured median 36 us for
//! connect-request-response, against 50,141 us when this loop polled on a
//! 50 ms timer), and each response reaches the wire in **one** write.
//!
//! ```no_run
//! use std::sync::Arc;
//! use pinion_rpc::RpcIngress;
//! use pinion_rpc_transport::{Exposure, UnixSocketTransport};
//!
//! // `ingress` comes from the shell's `on_rpc_ingress` hook.
//! fn mount(ingress: Arc<dyn RpcIngress>, expose_at_boot: bool) {
//!     // R-PR48 — the boot exposure is part of the bind, so a policy of
//!     // "bound but withdrawn" holds from the endpoint's first instant.
//!     let control = UnixSocketTransport::serve_with_exposure(
//!         "/run/user/1000/my-app.sock",
//!         ingress,
//!         if expose_at_boot { Exposure::Serving } else { Exposure::Withdrawn },
//!     )
//!     .expect("bind RPC socket");
//!     // Keep `control` alive for as long as the endpoint should exist.
//!     // Toggle exposure at runtime:
//!     control.set_exposure(Exposure::Withdrawn);
//!     control.set_exposure(Exposure::Serving);
//!     // Dropping `control` unbinds the socket and stops serving.
//!     std::mem::forget(control); // (example: hand ownership elsewhere)
//! }
//! ```

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::AsFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, poll};

use pinion_rpc::{ConnId, FnEgress, RpcEgress, RpcFrame, RpcIngress};

/// How long the accept loop backs off after an error it cannot act on.
///
/// R1541 — this is **not** an accept interval. The loop waits on readiness
/// (see [`accept_loop`]), so a connection is accepted the instant it arrives
/// and an idle endpoint does not wake at all; this constant is reached only
/// when the wait itself fails or `accept` returns an error other than
/// `WouldBlock`. Both leave the listener readable, so returning straight to
/// the wait would spin a core; parking briefly keeps the endpoint alive
/// through a transient exhaustion (`EMFILE`, `ENFILE`) instead of tearing it
/// down, at the cost of admitting no connection for this long while the
/// condition lasts.
const ERROR_BACKOFF: Duration = Duration::from_millis(50);

/// Index of the listener in the accept loop's poll set.
const POLL_LISTENER: usize = 0;
/// Index of the wake channel's read end in the accept loop's poll set.
const POLL_WAKE: usize = 1;

/// R1478 §5.7 — bind a listener at `path`, reclaiming the path only when no
/// live endpoint owns it.
///
/// The fixed-path endpoint has two indistinguishable-by-`stat` states to tell
/// apart, and only one syscall tells them apart: a **stale** socket file left
/// by a crashed run (nothing behind it) versus a **live** endpoint that is
/// serving right now. `bind` reports both as `EADDRINUSE`, so the ambiguity
/// is resolved by asking the path itself — a `connect` that succeeds means
/// somebody is listening.
///
/// The probe runs *only* on `EADDRINUSE`, so the ordinary case (a free path)
/// costs nothing and disturbs nobody. When it does run against a live
/// endpoint it costs that endpoint one accepted-and-immediately-closed
/// connection — an `on_connect` / `on_disconnect` pair carrying no frames.
/// That is the price of the only liveness test `AF_UNIX` offers without a
/// side-channel lock file, and it is paid only by a bind that was about to
/// displace something.
///
/// A `connect` against a [`Withdrawn`](Exposure::Withdrawn) endpoint also
/// succeeds — the listen backlog answers, not the accept loop — which is the
/// wanted answer: withdrawn refuses *service*, it still owns the *name*.
fn bind_unless_live(path: &Path) -> io::Result<UnixListener> {
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            if UnixStream::connect(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!(
                        "{} is held by a live RPC endpoint; refusing to displace it",
                        path.display()
                    ),
                ));
            }
            // Nobody is behind it: a genuine leftover, so reclaim the path.
            // A removal failure is propagated rather than swallowed — the
            // endpoint cannot exist, and the caller should hear why.
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            UnixListener::bind(path)
        }
        Err(e) => Err(e),
    }
}

/// R-PR48 §5.7 — whether a bound endpoint *serves* the connections it
/// accepts: the transport-policy state, declared at bind and toggleable at
/// runtime.
///
/// The socket is bound either way — that is what "always there,
/// execution-independent" means. `Withdrawn` is not "not listening": the
/// path exists and a client's `connect` still succeeds at the OS level, but
/// the server closes the connection without serving it, so an AI/agent path
/// can be withdrawn without the endpoint moving or the app restarting.
///
/// # Why an enum rather than a `bool`
///
/// Exposure is the vocabulary this crate already reasons in, and a
/// `serve_with_enabled(path, ingress, false)` call site says nothing about
/// what `false` withdraws. Naming it also keeps the *read* side symmetric
/// with the *write* side ([`TransportControl::exposure`] answers in the same
/// type the bind took), and leaves room for the axis to gain a state without
/// a `serve_with_enabled_and_…` explosion.
///
/// Deliberately **not** `#[non_exhaustive]`: if a third exposure state ever
/// lands, a consumer matching on this must be made to decide what its policy
/// does with it, rather than silently falling into a `_` arm — that
/// forced-decision property is the whole reason this is not a `bool`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Exposure {
    /// Accepted connections are served: frames flow to the [`RpcIngress`]
    /// and responses route back. The behaviour of a bare [`serve`].
    ///
    /// [`serve`]: UnixSocketTransport::serve
    #[default]
    Serving,
    /// Accepted connections are closed immediately — the endpoint is bound
    /// but refuses service.
    Withdrawn,
}

impl Exposure {
    /// Whether this exposure serves accepted connections.
    #[must_use]
    pub fn is_serving(self) -> bool {
        matches!(self, Self::Serving)
    }

    /// The `bool` view's inverse: `true` → [`Serving`](Self::Serving),
    /// `false` → [`Withdrawn`](Self::Withdrawn). The bridge for a consumer
    /// whose own policy is already a flag (an `APP_RPC=off` env read).
    #[must_use]
    pub fn from_serving(serving: bool) -> Self {
        if serving {
            Self::Serving
        } else {
            Self::Withdrawn
        }
    }
}

/// A Unix domain socket RPC transport built on the [`RpcIngress`] seam.
///
/// A unit type used only as the namespace for
/// [`serve_with_exposure`](Self::serve_with_exposure) and its
/// [`serve`](Self::serve) shorthand; the live state lives in the returned
/// [`TransportControl`].
pub struct UnixSocketTransport;

impl UnixSocketTransport {
    /// Bind a Unix domain socket at `path` and start serving JSON-RPC 2.0
    /// frames into `ingress`.
    ///
    /// Shorthand for
    /// [`serve_with_exposure(path, ingress, Exposure::Serving)`](Self::serve_with_exposure)
    /// — an endpoint that serves from its first instant. A consumer whose
    /// boot policy is "bound but withdrawn" must call
    /// [`serve_with_exposure`](Self::serve_with_exposure) instead of
    /// withdrawing afterwards; see there for why.
    ///
    /// # Errors
    /// As [`serve_with_exposure`](Self::serve_with_exposure).
    pub fn serve(
        path: impl AsRef<Path>,
        ingress: Arc<dyn RpcIngress>,
    ) -> io::Result<TransportControl> {
        Self::serve_with_exposure(path, ingress, Exposure::Serving)
    }

    /// R-PR48 §5.7 — bind a Unix domain socket at `path` **with the
    /// endpoint's initial [`Exposure`]**, and start serving JSON-RPC 2.0
    /// frames into `ingress`.
    ///
    /// Each accepted connection gets its own reader (line-delimited frames
    /// → [`RpcIngress::submit`]) and its own writer (responses back to
    /// that same connection), so a response always reaches the client that
    /// asked — unlike the single global stdout of the built-in transport.
    /// Multiple clients may connect concurrently.
    ///
    /// R-PR67 — each connection is given a fresh [`ConnId`]; every frame
    /// carries it, [`RpcIngress::on_connect`] fires when the connection
    /// opens, and [`RpcIngress::on_disconnect`] fires when it closes
    /// (crash-safe: however the client dies, its reader thread ends and
    /// signals). A stateful ingress uses these to keep per-connection state.
    ///
    /// R1478 — the path is the endpoint's **name**, and a name has one owner.
    /// A *stale* socket file left at `path` by a previous crashed run is
    /// reclaimed; a *live* endpoint already bound there is never displaced —
    /// the bind fails with [`io::ErrorKind::AddrInUse`] instead (see
    /// `bind_unless_live` for how the two are told apart, and why a
    /// withdrawn endpoint counts as live). Symmetrically, the name is
    /// released exactly once and only while this endpoint still holds it —
    /// see [`TransportControl::shutdown`].
    ///
    /// The returned [`TransportControl`] owns the endpoint's lifetime: keep it
    /// alive to keep serving, toggle [`TransportControl::set_exposure`] for
    /// runtime on/off, or drop it to unbind and stop.
    ///
    /// # Why exposure is an argument and not a post-bind call
    ///
    /// R-PR48 (raised by the sprag consumer, whose `APP_RPC=off` policy is
    /// "bind the socket, refuse service"): without this argument the only
    /// expressible sequence was `serve(..)` — which bound *serving* and
    /// armed the accept loop — followed by the consumer withdrawing once it
    /// got the control back. Everything the consumer does in between is
    /// window, and a client that lands in it is not exposed for that
    /// instant only: it is accepted *while serving*, and
    /// [`set_exposure`](TransportControl::set_exposure) deliberately leaves
    /// in-flight connections alone, so it is served for its **whole
    /// session** — by an endpoint the operator asked to be withdrawn.
    /// Taking the exposure here removes the window by construction: the
    /// flag the accept loop reads is created from `exposure` *before* the
    /// loop exists, and nothing writes it between the two.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] if the socket cannot be bound
    /// at `path` (permission, path too long, parent dir missing), if the
    /// listener cannot be set non-blocking, or if the accept thread cannot
    /// be spawned. R1478 — [`io::ErrorKind::AddrInUse`] specifically means a
    /// live endpoint already owns `path`.
    pub fn serve_with_exposure(
        path: impl AsRef<Path>,
        ingress: Arc<dyn RpcIngress>,
        exposure: Exposure,
    ) -> io::Result<TransportControl> {
        let path = path.as_ref().to_path_buf();
        let listener = bind_unless_live(&path)?;
        // Non-blocking accept: the loop waits on readiness and then drains
        // every queued connection, so it needs `accept` to *tell* it when the
        // queue is empty rather than blocking on the next arrival.
        listener.set_nonblocking(true)?;

        // R1541 — the out-of-band wake channel. `shutdown` closes the writing
        // half to end the accept loop's wait immediately. It is deliberately
        // NOT the socket path: a wake that had to reach the endpoint through
        // its own name could not happen after the unlink, which is where
        // R1478 requires the unlink to be (see `accept_loop`).
        let (wake_rx, wake_tx) = UnixStream::pair()?;

        // R-PR48 — the ONE construction site for the flag the accept loop
        // reads, and it is the declared exposure. "No window" is structural
        // rather than a rule to remember: no `TransportControl` exists until
        // the tail of this function, so nothing *can* write the flag between
        // its creation here and the loop being armed below.
        //
        // Honest limit: that is a structural guarantee, not a measured one.
        // An implementation that bound `Serving` and withdrew before
        // returning would pass every test in this crate — a sub-microsecond
        // internal window has no external witness. What the tests do pin is
        // the consumer-visible half, which is the half that bites:
        // `a_post_bind_withdraw_leaves_a_session_it_meant_to_refuse`.
        let serving = Arc::new(AtomicBool::new(exposure.is_serving()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let wakeups = Arc::new(AtomicU64::new(0));

        let accept = {
            let serving = Arc::clone(&serving);
            let shutdown = Arc::clone(&shutdown);
            let wakeups = Arc::clone(&wakeups);
            // R1478 — this thread deliberately does NOT unlink the path on its
            // way out. Releasing the name is `TransportControl::shutdown`'s
            // single responsibility, and it does it while the listener is
            // still open; see there for why that ordering is the invariant.
            thread::Builder::new()
                .name("pinion-rpc-unix-accept".to_owned())
                .spawn(move || {
                    accept_loop(&listener, &wake_rx, &ingress, &serving, &shutdown, &wakeups);
                })?
        };

        Ok(TransportControl {
            serving,
            shutdown,
            wakeups,
            wake: Some(wake_tx),
            path,
            accept: Some(accept),
        })
    }
}

/// A live handle to a running [`UnixSocketTransport`]: the consumer-owned
/// policy layer.
///
/// Holds the endpoint's lifetime. Dropping it (or calling
/// [`shutdown`](Self::shutdown)) unbinds the socket and stops the accept
/// loop; [`set_exposure`](Self::set_exposure) toggles whether new
/// connections are served without unbinding.
pub struct TransportControl {
    /// The flag the accept loop reads, as the `bool` view of [`Exposure`].
    /// Created once, from the exposure declared at bind (R-PR48).
    serving: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    wakeups: Arc<AtomicU64>,
    /// R1541 — the writing half of the accept loop's wake channel. Nothing is
    /// ever written to it: closing it is the signal, and the first
    /// [`shutdown`](TransportControl::shutdown) is what closes it.
    wake: Option<UnixStream>,
    path: PathBuf,
    /// The accept thread, and — because it is taken by the first
    /// [`shutdown`](TransportControl::shutdown) — this endpoint's "do I still
    /// own the name?" bit (R1478).
    accept: Option<JoinHandle<()>>,
}

impl TransportControl {
    /// R-PR48 — the endpoint's current [`Exposure`], in the same type the
    /// bind took. Read/write symmetry: whatever
    /// [`set_exposure`](Self::set_exposure) or
    /// [`serve_with_exposure`](UnixSocketTransport::serve_with_exposure)
    /// declared reads back here unchanged.
    #[must_use]
    pub fn exposure(&self) -> Exposure {
        Exposure::from_serving(self.serving.load(Ordering::Relaxed))
    }

    /// R-PR48 — expose or withdraw the endpoint at runtime. While
    /// [`Withdrawn`](Exposure::Withdrawn) the socket stays bound but every
    /// new connection is closed immediately (the endpoint refuses service),
    /// so an AI/agent path can be exposed or withdrawn without restarting
    /// the app.
    ///
    /// In-flight connections accepted while
    /// [`Serving`](Exposure::Serving) are **unaffected** — withdrawing
    /// refuses future admissions, it does not evict live sessions. That is
    /// also why the boot exposure belongs at the bind rather than here: a
    /// session admitted before the first withdraw outlives it (see
    /// [`serve_with_exposure`](UnixSocketTransport::serve_with_exposure)).
    pub fn set_exposure(&self, exposure: Exposure) {
        self.serving.store(exposure.is_serving(), Ordering::Relaxed);
    }

    /// The `bool` view of [`exposure`](Self::exposure): whether the
    /// endpoint is currently serving new connections.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.exposure().is_serving()
    }

    /// The `bool` view of [`set_exposure`](Self::set_exposure), for a
    /// consumer whose own policy is already a flag.
    pub fn set_enabled(&self, on: bool) {
        self.set_exposure(Exposure::from_serving(on));
    }

    /// Convenience for `set_exposure(Exposure::Serving)`.
    pub fn enable(&self) {
        self.set_exposure(Exposure::Serving);
    }

    /// Convenience for `set_exposure(Exposure::Withdrawn)`.
    pub fn disable(&self) {
        self.set_exposure(Exposure::Withdrawn);
    }

    /// The socket path this endpoint is bound to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// R1541 §5.7 — how many times the accept loop has returned from its
    /// wait since the endpoint was bound.
    ///
    /// The endpoint states its own accept-loop activity, which is what makes
    /// "the wait is demand-driven" an assertable property rather than a claim
    /// about the implementation: an idle endpoint reads `0` here however long
    /// it idles, one arrival reads `1`, and a whole idle lifetime followed by
    /// [`shutdown`](Self::shutdown) reads `1` — the shutdown's own knock.
    ///
    /// A timer-polled accept loop cannot satisfy any of those, because it
    /// wakes on the clock rather than on arrivals; that is precisely the
    /// defect this counter was introduced to pin (see `accept_loop`).
    /// Qt's `QLocalServer` publishes no equivalent — the readiness wait lives
    /// in `QSocketNotifier` inside the event loop, where a consumer cannot
    /// see it at all.
    #[must_use]
    pub fn accept_wakeups(&self) -> u64 {
        self.wakeups.load(Ordering::Relaxed)
    }

    /// Stop serving, unbind the socket, and join the accept thread.
    /// Idempotent; also run by [`Drop`].
    ///
    /// # R1478 — releasing the name exactly once, while still holding it
    ///
    /// Teardown used to unlink `path` twice: once as the accept thread wound
    /// down, once here after the join. Between those two moments the path was
    /// free, so a *successor* could bind it — and the second unlink then
    /// deleted the successor's socket, leaving a live app nobody could reach.
    ///
    /// The window is removed rather than detected. Detecting it would mean
    /// comparing the file's identity before unlinking, and `(dev, ino)` cannot
    /// carry that weight: a `tmpfs` — where these sockets live — hands the
    /// freed inode number straight back to the next `bind`, so a successor's
    /// socket file routinely presents the *same* inode the departed one had.
    /// A check that cannot fail is not a guard.
    ///
    /// So instead: unlink **before** signalling the accept loop, while the
    /// listener is still open. A successor attempting to bind at that moment
    /// meets a live endpoint and is refused (`bind_unless_live`), so nothing
    /// can be bound at `path` except this endpoint. After the unlink the path
    /// is free and this endpoint never touches it again — the taken
    /// [`JoinHandle`] is what makes "never again" hold across the idempotent
    /// second call and the [`Drop`] that follows an explicit `shutdown`.
    ///
    /// Connections already accepted are unaffected, matching
    /// [`set_exposure`](Self::set_exposure): releasing the name refuses future
    /// arrivals, it does not evict live sessions.
    pub fn shutdown(&mut self) {
        let Some(handle) = self.accept.take() else {
            // Already shut down: the name was released then, and whatever
            // holds it now is not ours to remove.
            return;
        };
        // Ordered deliberately — see the note above. Best-effort: an unlink
        // failure leaves a stale file, which the next bind reclaims. The
        // accept loop is parked in its wait right now, so the listener is
        // still open across this unlink, which is the invariant.
        let _ = fs::remove_file(&self.path);
        self.shutdown.store(true, Ordering::Relaxed);
        // R1541 — the flag is the state, this is its delivery: closing the
        // wake channel ends the accept loop's wait now, rather than at the
        // end of a poll interval. The signal is the *close* rather than a
        // byte written into it, because closing an fd cannot fail — a write
        // can, and a failed wake would leave the loop parked forever with
        // the join below waiting on it. Ordered after the store, so the loop
        // never observes the wake without the state that explains it.
        drop(self.wake.take());
        let _ = handle.join();
    }
}

impl Drop for TransportControl {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The accept thread body: wait until the listener has a connection or the
/// wake channel closes, admit everything queued, repeat until `shutdown`.
///
/// # R1541 §5.7 — the wait is on the listener, not on a clock
///
/// This loop used to leave the listener non-blocking and `sleep` a fixed 50 ms
/// on every `WouldBlock`, which put that interval between a connection
/// arriving and the server accepting it. The constant's own docstring called
/// the cost negligible **for an out-of-band endpoint whose connections are
/// long-lived** — two premises, neither of them a property of this crate.
/// The sprag consumer falsifies both: this socket is the primary path its
/// agent tooling drives, and its CLI is a process per invocation, so every
/// call pays the interval in full with nothing to amortise it over. Measured
/// there (2026-08-02): 0.025 ms per request on a warm connection against
/// **50.188 ms** on a cold one, same server, same request, same box — 99.5%
/// of a CLI invocation's wall time was this constant. Reproduced here at
/// 50.141 ms before the fix (`tests/accept_wakes_on_arrival.rs`).
///
/// Waiting on readiness removes the interval rather than shrinking it: a
/// connection is admitted as soon as the kernel has one, an idle endpoint
/// makes no syscalls at all, and `shutdown` ends the loop immediately. All
/// three are asserted as counts rather than durations — see
/// [`TransportControl::accept_wakeups`].
///
/// ## Why not a blocking `accept` woken by a self-connect
///
/// The obvious dependency-free alternative, and the one the old docstring
/// named. It is rejected on a concrete conflict, not on taste: R1478 requires
/// the path to be unlinked **while the listener is still open**, so nothing
/// can bind at it between this endpoint releasing the name and its listener
/// closing. A self-connect wake has to reach the endpoint *through that same
/// path*, so it must happen before the unlink — which puts an unbounded
/// `connect` (it blocks when the listen backlog is full) inside `Drop`, and
/// re-opens the successor window R1478 closed. An out-of-band wake channel
/// has neither problem: it does not go through the name, so the unlink stays
/// exactly where R1478 put it.
///
/// ## Why `poll(2)` and not an event-loop crate
///
/// Two fds, one of them a socketpair used once. `poll` states that directly
/// and costs no registration; `polling` and `mio` are readiness *frameworks*
/// whose portability buys nothing for a type named `UnixSocketTransport`, and
/// `polling`'s `add` is an `unsafe fn` this workspace forbids outright.
/// [`rustix`] is a safe wrapper over the syscall, so no `unsafe` appears here
/// or in the dependency's public API.
fn accept_loop(
    listener: &UnixListener,
    wake: &UnixStream,
    ingress: &Arc<dyn RpcIngress>,
    serving: &Arc<AtomicBool>,
    shutdown: &Arc<AtomicBool>,
    wakeups: &Arc<AtomicU64>,
) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let mut fds = [
            PollFd::from_borrowed_fd(listener.as_fd(), PollFlags::IN),
            PollFd::from_borrowed_fd(wake.as_fd(), PollFlags::IN),
        ];
        // `None` is an unbounded wait: the thread is parked in the kernel
        // until one of the two fds has something, so an idle endpoint costs
        // nothing at all — not a syscall, not a scheduler slot.
        match poll(&mut fds, None) {
            Ok(_) => {}
            // An interrupted wait delivered no event and never completed, so
            // it is not a wakeup: re-enter the same wait (re-reading
            // `shutdown` on the way, which is why this `continue`s rather
            // than retrying in place).
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => {
                // The wait itself failed. Counted, because unlike `INTR` the
                // loop really did run — a counter that hid this would report
                // an idle endpoint while a core burned.
                wakeups.fetch_add(1, Ordering::Relaxed);
                thread::sleep(ERROR_BACKOFF);
                continue;
            }
        }
        wakeups.fetch_add(1, Ordering::Relaxed);

        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        if !fds[POLL_WAKE].revents().is_empty() {
            // Nothing ever *writes* to the wake channel, so its readiness can
            // only mean its writing half is gone — the control was destroyed
            // without setting the flag above. Unreachable through the public
            // API (`Drop` runs `shutdown`, which stores the flag before it
            // closes the channel), but a level-triggered fd that stays ready
            // would spin a core forever, so the impossible state exits rather
            // than being assumed away.
            break;
        }
        if fds[POLL_LISTENER].revents().is_empty() {
            continue;
        }

        // Admit everything the kernel has queued before returning to the
        // wait, rather than one connection per wakeup. Not load-bearing for
        // correctness — a level-triggered wait would simply return again for
        // the next one — and deliberately not asserted, because how many of a
        // burst have landed by the time the drain runs is a timing question
        // no zero-flake test can pin.
        loop {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    if serving.load(Ordering::Relaxed) {
                        let ingress = Arc::clone(ingress);
                        // Detached: the handler ends on its own when the
                        // client closes. A spawn failure drops the connection
                        // rather than aborting the whole endpoint.
                        let _ = thread::Builder::new()
                            .name("pinion-rpc-unix-conn".to_owned())
                            .spawn(move || handle_connection(&stream, &ingress));
                    }
                    // Withdrawn: drop `stream` — the connection is closed,
                    // the endpoint refuses service while off.
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    // Transient accept error (descriptor exhaustion is the
                    // realistic one): back off and keep serving rather than
                    // tearing the endpoint down. See `ERROR_BACKOFF`.
                    thread::sleep(ERROR_BACKOFF);
                    break;
                }
            }
        }
    }
}

/// R1541 §5.7 — one connection's reply stream: each response reaches `sink`
/// line-terminated, in **one** write call, until the channel closes or the
/// sink refuses.
///
/// Taking the sink by generic rather than writing to the socket in place is
/// what makes "one frame, one write" observable: the property lives in the
/// number of calls, so the only place it can be asserted is the [`Write`]
/// impl on the other side (see `one_response_is_one_write`).
///
/// The previous form, `writeln!(socket, "{response}")`, issued the payload and
/// the terminator as two separate syscalls — measured, not inferred:
/// `write(8, "echo:{\"jsonrpc\"...", 55)` then `write(8, "\n", 1)`. That costs
/// the client an extra readable event per response as well as costing this
/// side the second call. Appending to the `String` the channel already handed
/// us makes the frame one buffer, hence one call, **at any size**. A
/// [`BufWriter`](io::BufWriter) would not: a response larger than its capacity
/// bypasses the buffer and leaves the terminator behind for a second write,
/// and `scene/snapshot` replies routinely exceed any such capacity.
fn write_replies(mut sink: impl Write, rx: &mpsc::Receiver<String>) {
    while let Ok(mut response) = rx.recv() {
        response.push('\n');
        if sink.write_all(response.as_bytes()).is_err() {
            break;
        }
        // No `flush`: the sink is the unbuffered socket, so `write_all` has
        // already put the frame on the wire. Flushing here would be the
        // no-op that made the two-syscall framing above look free.
    }
}

/// Serve one connection: allocate its [`ConnId`], announce
/// [`RpcIngress::on_connect`], read line-delimited JSON-RPC frames and
/// submit each (stamped with that id) through `ingress` with a reply that
/// routes the response back to *this* connection's writer, and — however
/// the client goes away — announce [`RpcIngress::on_disconnect`] when the
/// reader loop ends. The disconnect signal is guaranteed because the reader
/// thread always reaches the end of this function when the client closes,
/// resets, or crashes (R-PR67 crash-safe cleanup).
fn handle_connection(stream: &UnixStream, ingress: &Arc<dyn RpcIngress>) {
    // The reader half blocks on incoming lines; the accepted stream was
    // set non-blocking by the listener, so restore blocking for clean
    // line reads.
    let _ = stream.set_nonblocking(false);
    let Ok(write_half) = stream.try_clone() else {
        return;
    };
    let Ok(read_half) = stream.try_clone() else {
        return;
    };

    // Per-connection writer: a single thread owns the write half and
    // serializes every response, so concurrent in-flight replies never
    // interleave bytes on the wire.
    let (tx, rx) = mpsc::channel::<String>();
    let writer = thread::Builder::new()
        .name("pinion-rpc-unix-write".to_owned())
        .spawn(move || write_replies(write_half, &rx));

    // R-PR67 — a fresh, process-unique id for this connection. Its open is
    // announced to the ingress before any frame; every frame is stamped
    // with it; its close is announced when the reader loop ends below.
    // Allocated after the `try_clone` guards so an early return there
    // leaves `on_connect` / `on_disconnect` balanced (neither fires).
    let conn = ConnId::allocate();
    ingress.on_connect(conn);

    // R1552 §5.7 PINION-PR83 — this connection's egress: the writer, named.
    // Built ONCE for the connection rather than once per frame, because a
    // subscription outlives the frame that opened it and keeps writing here.
    // Every frame's reply is derived from it (`RpcFrame::new` →
    // `RpcReply::over`), so a response and a `scene/changed` notification go
    // down the same channel to the same writer thread — they cannot interleave
    // mid-frame and cannot be reordered against each other.
    let egress: Arc<dyn RpcEgress> = FnEgress::new(move |frame: String| {
        // The writer is gone once the client closed mid-flight. `false` is what
        // prunes a subscription whose peer has vanished without waiting for
        // `on_disconnect` — the two agree, and either alone would do.
        tx.send(frame).is_ok()
    });

    let reader = BufReader::new(read_half);
    for line in reader.lines() {
        let Ok(text) = line else {
            break;
        };
        if text.trim().is_empty() {
            continue;
        }
        ingress.submit(RpcFrame::new(conn, text, Arc::clone(&egress)));
    }

    // R-PR67 — the reader loop ended: the client closed its write side
    // (EOF), reset, or crashed. Announce the disconnect immediately — this
    // is the crash-safe cleanup signal, fired however the client went away,
    // and it happens-after every `submit` for `conn` above — before draining
    // the writer, so a stateful ingress releases the connection's state
    // within a bounded time of the client vanishing (the writer teardown
    // that follows only flushes already-produced replies).
    ingress.on_disconnect(conn);

    // Client closed the read side. Drop this connection's egress so the
    // writer's channel closes once every in-flight reply — and every
    // subscription still holding a clone — has fired or been released, then
    // wait for the writer to flush and exit.
    //
    // R1552 — the release of the subscription clones is what `on_disconnect`
    // above just did: an egress clone IS per-connection state, which is the
    // state that hook's contract already requires an ingress to release. An
    // ingress that kept one past its disconnect would park this writer thread
    // forever, which is why the ordering here is disconnect-then-drop.
    drop(egress);
    if let Ok(handle) = writer {
        let _ = handle.join();
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::mpsc;

    use super::{Exposure, write_replies};

    /// A sink that accepts everything and remembers how it was *called*, not
    /// merely what it received. The call boundaries are the property under
    /// test; a sink that only accumulated bytes would report the same
    /// `Vec<u8>` however many syscalls produced it.
    #[derive(Default)]
    struct CountingSink {
        calls: Vec<Vec<u8>>,
    }

    impl Write for CountingSink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.calls.push(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Feed `responses` through [`write_replies`] and return the calls the
    /// sink saw. The channel is closed before the drain runs, so the loop
    /// terminates on its own and nothing here depends on timing.
    fn replies_written(responses: &[&str]) -> Vec<Vec<u8>> {
        let (tx, rx) = mpsc::channel::<String>();
        for r in responses {
            tx.send((*r).to_owned()).unwrap();
        }
        drop(tx);
        let mut sink = CountingSink::default();
        write_replies(&mut sink, &rx);
        sink.calls
    }

    #[test]
    fn one_response_is_one_write() {
        // R1541 — the framing costs one call per frame. `writeln!` on an
        // unbuffered socket cost two (payload, then terminator), which is
        // invisible in the bytes and shows up only in the call count: this
        // asserts `2`, and the pre-R1541 form produced `4`.
        let calls = replies_written(&["first", "second"]);
        assert_eq!(
            calls.len(),
            2,
            "one write per response, terminator included"
        );
        assert_eq!(calls[0], b"first\n");
        assert_eq!(calls[1], b"second\n");
    }

    #[test]
    fn a_large_response_is_still_one_write() {
        // The case that rules out a `BufWriter`: a frame larger than any
        // fixed capacity still leaves in a single call, because the buffer
        // IS the frame rather than a fixed window onto it. 64 KiB is past
        // `BufWriter`'s 8 KiB default by a factor of eight.
        let big = "x".repeat(64 * 1024);
        let calls = replies_written(&[&big]);
        assert_eq!(calls.len(), 1, "a 64 KiB frame is not split by the framing");
        assert_eq!(calls[0].len(), big.len() + 1);
        assert_eq!(calls[0].last(), Some(&b'\n'));
    }

    #[test]
    fn an_empty_response_still_carries_its_terminator() {
        // A degenerate frame is still a frame: the reader is line-delimited,
        // so a zero-length response that dropped its newline would make the
        // client block on the *next* response's bytes to end this one.
        let calls = replies_written(&[""]);
        assert_eq!(calls, vec![b"\n".to_vec()]);
    }

    #[test]
    fn exposure_round_trips_through_its_bool_view() {
        // The bridge a consumer's own flag crosses (`APP_RPC=off` → policy).
        // A transposition here would silently invert every consumer's boot
        // policy while every other test still passed, so it is pinned in both
        // directions rather than read off the definition.
        assert!(Exposure::Serving.is_serving());
        assert!(!Exposure::Withdrawn.is_serving());
        assert_eq!(Exposure::from_serving(true), Exposure::Serving);
        assert_eq!(Exposure::from_serving(false), Exposure::Withdrawn);
        for exposure in [Exposure::Serving, Exposure::Withdrawn] {
            assert_eq!(Exposure::from_serving(exposure.is_serving()), exposure);
        }
    }

    #[test]
    fn the_default_exposure_is_the_bare_serve_behaviour() {
        // `serve` is the `Exposure::Serving` shorthand, so a consumer that
        // falls back to `Exposure::default()` (an unset env var) lands on the
        // pre-PR48 behaviour rather than silently withdrawing its endpoint.
        assert_eq!(Exposure::default(), Exposure::Serving);
    }
}
