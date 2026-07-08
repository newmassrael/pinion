//! §5.7 PR-47 — transport-injection seam decoupling RPC ingress/egress
//! from the process standard streams.
//!
//! The dispatch core ([`crate::dispatch_parsed`], and the shell's
//! `ShellCore::dispatch_rpc*` that wraps it) is already transport-agnostic:
//! it takes a request string and *returns* the response string. What was
//! hard-wired was the *transport* — the shell read frames off
//! `std::io::stdin` and wrote responses to `std::io::stdout`, so an RPC
//! endpoint could only exist where the parent process happened to wire fd
//! 0 / fd 1. That made an "always there, execution-independent" endpoint
//! and runtime on/off impossible (fd 0 is fixed at exec time).
//!
//! This module is the winit-free contract that lets any producer submit a
//! frame and receive the response back on the same transport it arrived
//! on:
//!
//! - [`RpcReply`] — a one-shot sink for a single response string. The
//!   producer decides where the response goes (stdout for the built-in
//!   stdin reader, a socket connection for `pinion-rpc-transport`, a test
//!   channel, ...). Dispatch calls [`RpcReply::send`] exactly once when
//!   there is a response; a JSON-RPC notification (no `id`, hence no
//!   response) simply drops the reply unused.
//! - [`RpcFrame`] — one request paired with its reply sink.
//! - [`RpcIngress`] — the winit-free handle a producer uses to hand a
//!   frame to the UI thread. The GUI backend wraps a winit
//!   `EventLoopProxy`; the TUI backend wraps an `mpsc::Sender`; neither
//!   winit nor crossterm leaks across this boundary, preserving the §2 #6
//!   GUI/TUI dual invariant. Exposing the raw `EventLoopProxy` instead
//!   would be un-implementable for the TUI backend (which has no winit
//!   event loop to build a proxy from) — the winit-free trait is what
//!   makes one seam serve both.

use std::fmt;

/// A one-shot sink for a single JSON-RPC response string, routed back to
/// the transport the request arrived on.
///
/// Wraps a `FnOnce` rather than a channel so the built-in stdin producer
/// can supply a plain "write to stdout" closure without allocating a
/// channel + drain thread, while a socket producer supplies a closure
/// that forwards to its per-connection writer. The dispatch layer calls
/// [`send`](Self::send) at most once (never, for a notification that
/// produced no response — the reply is then dropped).
///
/// `Send` (not `Sync`): the reply travels from the producer thread into
/// the UI thread inside [`RpcFrame`], where dispatch invokes it; it is
/// never shared by reference across threads.
pub struct RpcReply(Box<dyn FnOnce(String) + Send>);

impl RpcReply {
    /// Build a reply that runs `sink` with the response string when
    /// dispatch produces one.
    #[must_use]
    pub fn new(sink: impl FnOnce(String) + Send + 'static) -> Self {
        Self(Box::new(sink))
    }

    /// Deliver the one response for this frame. Consumes the reply so it
    /// can fire at most once.
    pub fn send(self, response: String) {
        (self.0)(response);
    }
}

impl fmt::Debug for RpcReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The boxed closure is not itself `Debug`; a stable placeholder
        // keeps `#[derive(Debug)]` working on the enclosing `RpcFrame`
        // (and the shell's `AppEvent`, which embeds it).
        f.write_str("RpcReply(..)")
    }
}

/// One JSON-RPC 2.0 request paired with the reply sink that routes its
/// response back to the originating transport.
///
/// Replaces the bare request `String` the shell used to move across the
/// UI-thread boundary: carrying the reply with the request is what lets a
/// response reach a *specific* connection instead of the single global
/// stdout, which is the prerequisite for any multi-connection transport.
#[derive(Debug)]
pub struct RpcFrame {
    /// The raw JSON-RPC 2.0 envelope, one frame, not yet parsed.
    pub request: String,
    /// Where this frame's response is delivered (once, if any).
    pub reply: RpcReply,
}

impl RpcFrame {
    /// Pair a raw request envelope with its reply sink.
    #[must_use]
    pub fn new(request: String, reply: RpcReply) -> Self {
        Self { request, reply }
    }
}

/// Winit-free handle a transport uses to submit a frame to the UI thread
/// for dispatch.
///
/// Implemented once per backend inside that backend's crate (the GUI
/// wrapper over `winit::EventLoopProxy` in `pinion-shell`, the TUI wrapper
/// over `std::sync::mpsc::Sender` in `pinion-tui`) so that no
/// backend-specific type crosses this boundary. A transport adapter (e.g.
/// a Unix-socket listener) holds an `Arc<dyn RpcIngress>` and calls
/// [`submit`](Self::submit) for every frame it reads; the same handle is
/// used by the built-in stdin reader, so all producers share one ingress
/// path into the identical dispatch core.
pub trait RpcIngress: Send + Sync {
    /// Hand one frame to the UI thread for dispatch. Non-blocking: the
    /// frame is queued (winit user-event / mpsc channel) and dispatched
    /// on the next UI-thread turn. A frame submitted after the UI loop
    /// has shut down is dropped (its reply never fires).
    fn submit(&self, frame: RpcFrame);
}
