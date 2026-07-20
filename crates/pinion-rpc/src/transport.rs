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
//! - [`RpcFrame`] — one request paired with its reply sink, tagged with
//!   the [`ConnId`] of the connection it arrived on (R-PR67).
//! - [`RpcIngress`] — the winit-free handle a producer uses to hand a
//!   frame to the UI thread, plus the R-PR67 connection-lifecycle hooks
//!   ([`on_connect`](RpcIngress::on_connect) /
//!   [`on_disconnect`](RpcIngress::on_disconnect)) that let a stateful
//!   ingress track per-connection state crash-safely. The GUI backend wraps
//!   a winit
//!   `EventLoopProxy`; the TUI backend wraps an `mpsc::Sender`; neither
//!   winit nor crossterm leaks across this boundary, preserving the §2 #6
//!   GUI/TUI dual invariant. Exposing the raw `EventLoopProxy` instead
//!   would be un-implementable for the TUI backend (which has no winit
//!   event loop to build a proxy from) — the winit-free trait is what
//!   makes one seam serve both.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// R-PR67 §5.7 — an opaque, process-unique lifecycle token for one
/// transport connection.
///
/// A transport issues a fresh `ConnId` when a connection opens
/// ([`allocate`](Self::allocate)), stamps every [`RpcFrame`] it reads from
/// that connection with it, and passes it to the
/// [`RpcIngress::on_connect`] / [`RpcIngress::on_disconnect`] lifecycle
/// hooks. That lets a *stateful* ingress — one dispatch owner serving many
/// connections — attribute a frame to its originating connection and pair
/// each disconnect with the frames that preceded it, the prerequisite for
/// any per-connection server state (in-flight transactions, per-connection
/// rate limits, "who is attached").
///
/// It is a **lifecycle** token, not identity: it says "these frames came
/// from the same still-open connection", nothing about who the peer is.
/// Ids are monotonic and never reused within a process (a fresh 64-bit
/// counter), so a value that has disconnected can never collide with a
/// later connection. Uniqueness spans every transport in the process — one
/// stdin reader and several socket connections each get a distinct id —
/// because they all draw from the same counter.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ConnId(u64);

impl ConnId {
    /// Allocate a fresh, process-unique connection id. A transport calls
    /// this once per connection open — once, for the single logical
    /// connection of a stdin reader; once per accepted socket connection.
    #[must_use]
    pub fn allocate() -> Self {
        // Starts at 1 so a bare `ConnId(0)` is never a live id (some
        // consumers reserve 0 as a "none" sentinel). Relaxed is enough: the
        // only invariant is uniqueness, and `fetch_add` is atomic on its
        // own; no other memory is ordered against the id.
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// The raw monotonic value, for a consumer that keys a `conn -> state`
    /// map on it or logs it. Opaque: no meaning beyond identity + ordering.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

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
/// response back to the originating transport, tagged with the [`ConnId`]
/// of the connection it arrived on.
///
/// Replaces the bare request `String` the shell used to move across the
/// UI-thread boundary: carrying the reply with the request is what lets a
/// response reach a *specific* connection instead of the single global
/// stdout, and carrying the [`ConnId`] (R-PR67) is what lets a stateful
/// ingress attribute the frame to that connection — both prerequisites for
/// any multi-connection transport.
#[derive(Debug)]
pub struct RpcFrame {
    /// The connection this frame arrived on (R-PR67). A stateful ingress
    /// uses it to attribute the frame to its originating connection and to
    /// match the later [`RpcIngress::on_disconnect`] for that connection; a
    /// stateless ingress ignores it. The built-in stdin transport stamps
    /// every frame with its own single, stable id.
    pub conn: ConnId,
    /// The raw JSON-RPC 2.0 envelope, one frame, not yet parsed.
    pub request: String,
    /// Where this frame's response is delivered (once, if any).
    pub reply: RpcReply,
}

impl RpcFrame {
    /// Pair a raw request envelope with its reply sink, tagged with the
    /// [`ConnId`] of the connection it arrived on.
    #[must_use]
    pub fn new(conn: ConnId, request: String, reply: RpcReply) -> Self {
        Self {
            conn,
            request,
            reply,
        }
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

    /// R-PR67 — called once when a connection opens, before any of its
    /// frames are submitted. Default: no-op, so a stateless ingress needs
    /// no change. A stateful ingress overrides it to begin tracking the
    /// connection (the `+1` half of a "who is attached" count).
    fn on_connect(&self, conn: ConnId) {
        let _ = conn;
    }

    /// R-PR67 — called once when a connection closes: EOF, reset, or the
    /// client crashing. This is the **crash-safe cleanup hook** — however
    /// the client goes away, the transport's per-connection reader ends and
    /// calls this, so a stateful ingress can release the connection's state
    /// (the `-1` half of a "who is attached" count) even when no explicit
    /// `detach` message ever arrives. Default: no-op.
    ///
    /// Ordering: for a given `conn`, every [`submit`](Self::submit) of a
    /// frame carrying that `conn` happens-before this call, in program
    /// order on the transport's reader thread. An ingress that routes both
    /// its frames and this signal onto one FIFO queue therefore observes
    /// the disconnect strictly after all of that connection's frames.
    fn on_disconnect(&self, conn: ConnId) {
        let _ = conn;
    }
}
