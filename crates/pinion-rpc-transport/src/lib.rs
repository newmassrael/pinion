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
//!   TUI, because it depends only on the seam.
//! - **Transport policy** (when the endpoint is on/off, where it lives) —
//!   owned by the *consumer*: it chooses the path, holds the
//!   [`TransportControl`], and toggles [`TransportControl::set_enabled`] or
//!   drops the control to tear the endpoint down. There is no
//!   framework-side toggle mechanism; runtime on/off falls out of the
//!   consumer owning the transport's lifetime.
//!
//! ```no_run
//! use std::sync::Arc;
//! use pinion_rpc::RpcIngress;
//! use pinion_rpc_transport::UnixSocketTransport;
//!
//! // `ingress` comes from the shell's `on_rpc_ingress` hook.
//! fn mount(ingress: Arc<dyn RpcIngress>) {
//!     let control = UnixSocketTransport::serve("/run/user/1000/my-app.sock", ingress)
//!         .expect("bind RPC socket");
//!     // Keep `control` alive for as long as the endpoint should exist.
//!     // Toggle exposure at runtime:
//!     control.disable();
//!     control.enable();
//!     // Dropping `control` unbinds the socket and stops serving.
//!     std::mem::forget(control); // (example: hand ownership elsewhere)
//! }
//! ```

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use pinion_rpc::{RpcFrame, RpcIngress, RpcReply};

/// How long the accept loop parks between `WouldBlock` polls of the
/// non-blocking listener.
///
/// This is a control-plane socket (RPC drive / introspection), never the
/// render hot path, so a small fixed poll — rather than a blocking accept
/// woken by a self-connect trick, or an event-loop dependency like `mio` —
/// is the clearer, dependency-free choice. The cost is up to this much
/// latency before a freshly-arrived connection is accepted, and ~20
/// idle wakeups/second on the dedicated accept thread; both are negligible
/// for an out-of-band endpoint. It mirrors the adaptive-poll pattern the
/// TUI shell already uses for its own event source.
const ACCEPT_POLL: Duration = Duration::from_millis(50);

/// A Unix domain socket RPC transport built on the [`RpcIngress`] seam.
///
/// A unit type used only as the namespace for [`serve`](Self::serve); the
/// live state lives in the returned [`TransportControl`].
pub struct UnixSocketTransport;

impl UnixSocketTransport {
    /// Bind a Unix domain socket at `path` and start serving JSON-RPC 2.0
    /// frames into `ingress`.
    ///
    /// Each accepted connection gets its own reader (line-delimited frames
    /// → [`RpcIngress::submit`]) and its own writer (responses back to
    /// that same connection), so a response always reaches the client that
    /// asked — unlike the single global stdout of the built-in transport.
    /// Multiple clients may connect concurrently.
    ///
    /// A stale socket file left at `path` by a previous crashed run is
    /// removed before binding. The returned [`TransportControl`] owns the
    /// endpoint's lifetime: keep it alive to keep serving, toggle
    /// [`TransportControl::set_enabled`] for runtime on/off, or drop it to
    /// unbind and stop.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] if the socket cannot be bound
    /// at `path` (permission, path too long, parent dir missing), if the
    /// listener cannot be set non-blocking, or if the accept thread cannot
    /// be spawned.
    pub fn serve(
        path: impl AsRef<Path>,
        ingress: Arc<dyn RpcIngress>,
    ) -> io::Result<TransportControl> {
        let path = path.as_ref().to_path_buf();
        // A leftover socket file from a crashed run would make `bind` fail
        // with EADDRINUSE; clearing it first makes a fixed-path endpoint
        // reliably re-bindable across restarts.
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        // Non-blocking accept so the loop can observe `shutdown` between
        // polls without a blocked syscall pinning the thread.
        listener.set_nonblocking(true)?;

        let enabled = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(AtomicBool::new(false));

        let accept = {
            let enabled = Arc::clone(&enabled);
            let shutdown = Arc::clone(&shutdown);
            let path = path.clone();
            thread::Builder::new()
                .name("pinion-rpc-unix-accept".to_owned())
                .spawn(move || {
                    accept_loop(&listener, &ingress, &enabled, &shutdown);
                    // Best-effort: remove the socket file when serving
                    // ends so the path is free for the next bind.
                    let _ = fs::remove_file(&path);
                })?
        };

        Ok(TransportControl {
            enabled,
            shutdown,
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
/// loop; [`set_enabled`](Self::set_enabled) toggles whether new
/// connections are served without unbinding.
pub struct TransportControl {
    enabled: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    path: PathBuf,
    accept: Option<JoinHandle<()>>,
}

impl TransportControl {
    /// Whether the endpoint is currently serving new connections.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Turn serving on or off at runtime. While disabled the socket stays
    /// bound but every new connection is closed immediately (the endpoint
    /// refuses service), so an AI/agent path can be exposed or withdrawn
    /// without restarting the app. In-flight connections accepted while
    /// enabled are unaffected.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// Convenience for `set_enabled(true)`.
    pub fn enable(&self) {
        self.set_enabled(true);
    }

    /// Convenience for `set_enabled(false)`.
    pub fn disable(&self) {
        self.set_enabled(false);
    }

    /// The socket path this endpoint is bound to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Stop serving, unbind the socket, and join the accept thread.
    /// Idempotent; also run by [`Drop`].
    pub fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.accept.take() {
            // The accept loop observes `shutdown` within one `ACCEPT_POLL`
            // and then removes the socket file itself.
            let _ = handle.join();
        }
        // Best-effort in case the accept thread never spawned / already
        // exited; removing an absent file is harmless.
        let _ = fs::remove_file(&self.path);
    }
}

impl Drop for TransportControl {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The accept thread body: poll the non-blocking listener, dispatch each
/// connection to its own handler thread, until `shutdown` is set.
fn accept_loop(
    listener: &UnixListener,
    ingress: &Arc<dyn RpcIngress>,
    enabled: &Arc<AtomicBool>,
    shutdown: &Arc<AtomicBool>,
) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match listener.accept() {
            Ok((stream, _addr)) => {
                if enabled.load(Ordering::Relaxed) {
                    let ingress = Arc::clone(ingress);
                    // Detached: the handler ends on its own when the client
                    // closes. A spawn failure drops the connection rather
                    // than aborting the whole endpoint.
                    let _ = thread::Builder::new()
                        .name("pinion-rpc-unix-conn".to_owned())
                        .spawn(move || handle_connection(&stream, &ingress));
                }
                // Disabled: drop `stream` — the connection is closed, the
                // endpoint refuses service while off.
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => {
                // Transient accept error: back off briefly and keep
                // serving rather than tearing the endpoint down.
                thread::sleep(ACCEPT_POLL);
            }
        }
    }
}

/// Serve one connection: read line-delimited JSON-RPC frames, submit each
/// through `ingress` with a reply that routes the response back to *this*
/// connection's writer, until the client closes.
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
        .spawn(move || {
            let mut w = write_half;
            while let Ok(response) = rx.recv() {
                if writeln!(w, "{response}").is_err() {
                    break;
                }
                let _ = w.flush();
            }
        });

    let reader = BufReader::new(read_half);
    for line in reader.lines() {
        let Ok(text) = line else {
            break;
        };
        if text.trim().is_empty() {
            continue;
        }
        let reply_tx = tx.clone();
        let reply = RpcReply::new(move |response| {
            // Writer may already be gone if the client closed mid-flight;
            // a failed send is then simply dropped.
            let _ = reply_tx.send(response);
        });
        ingress.submit(RpcFrame::new(text, reply));
    }

    // Client closed the read side. Drop this frame-submitting `tx` so the
    // writer's channel closes once every in-flight reply clone has fired
    // or been dropped, then wait for the writer to flush and exit.
    drop(tx);
    if let Ok(handle) = writer {
        let _ = handle.join();
    }
}
