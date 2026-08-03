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
//! - [`RpcEgress`] — the connection's *writer*: one frame out, whether or
//!   not anybody asked for it (R1552 PINION-PR83). The mirror of
//!   [`RpcIngress`] below, and the primitive [`RpcReply`] is built from.
//! - [`RpcReply`] — a one-shot sink for a single response string. The
//!   producer decides where the response goes (stdout for the built-in
//!   stdin reader, a socket connection for `pinion-rpc-transport`, a test
//!   channel, ...). Dispatch calls [`RpcReply::send`] exactly once when
//!   there is a response; a JSON-RPC notification (no `id`, hence no
//!   response) simply drops the reply unused.
//! - [`RpcFrame`] — one request paired with its reply sink AND the egress
//!   of the connection it arrived on, tagged with that connection's
//!   [`ConnId`] (R-PR67).
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
use std::sync::Arc;
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

/// R1552 §5.7 PINION-PR83 — one connection's **writer**: a frame out,
/// whether or not anything asked for it.
///
/// The mirror of [`RpcIngress`]. Ingress carries frames *in* and is
/// implemented once per backend; egress carries frames *out* and is
/// implemented once per **transport**, because the writer is a transport
/// fact — the socket transport owns a per-connection writer thread, the
/// stdin reader owns the process's stdout, the TUI owns stderr. A backend
/// has no writer of its own to offer, which is why this is not a method on
/// `RpcIngress` (see the module docs).
///
/// # Why the framework needs one at all
///
/// Without it a request is the only thing that can produce a frame, so
/// **one request can produce at most one response** and a subscription — one
/// request, many answers — is not expressible on this transport at any
/// price. That is the shape PINION-PR83 reported: a consumer wanting a
/// change stream had to re-issue a request per batch, paying a round trip
/// each time. [`mod@crate::subscribe`] is the framework's own consumer.
///
/// # What may be written
///
/// A JSON-RPC 2.0 **notification** — a frame with a `method` and no `id`
/// (JSON-RPC 2.0, section 4.1). Deliberately *not* a second Response carrying the
/// originating request's `id`: the spec pairs one Response to one Request,
/// and every client keyed on that pairing (a pending-id map, including this
/// project's own `tools/rpc_verify.py`) discards the second one. A
/// notification is the form a client can tell apart from its own answer,
/// which is the only form a stream can arrive in without corrupting the
/// request/response channel it shares.
///
/// `Send + Sync`: an egress is cloned into a registry and written from
/// whichever thread produced the event — for [`mod@crate::subscribe`], the
/// thread that bumped the [`pinion_core::SceneRevision`].
pub trait RpcEgress: Send + Sync {
    /// Write one complete JSON-RPC frame to this connection.
    ///
    /// Returns `false` when the frame could not be handed to the
    /// connection's writer — the client is gone, the pipe is broken, the
    /// writer thread has ended. A caller holding a long-lived egress (a
    /// subscription) uses that to prune itself; it is a *report*, not an
    /// error, because a peer disappearing is ordinary.
    ///
    /// Implementations must not block on the client: the socket transport
    /// hands off to its writer thread, the stdin reader writes to an
    /// already-buffered stdout. A publish walk holds a registry lock in the
    /// general case, so a blocking egress would stall every other
    /// subscriber.
    fn send_frame(&self, frame: String) -> bool;

    /// Whether frames written here can reach a peer at all.
    ///
    /// `true` for every real transport. [`NullEgress`] answers `false`, which is
    /// what lets `scene/subscribe` **refuse** a frame that has no connection
    /// behind it instead of registering a stream whose notifications go nowhere
    /// — a client that believes it is subscribed and hears nothing cannot tell
    /// that from a scene that never changed.
    ///
    /// A capability question, not a liveness one: it does not become `false`
    /// when the peer later disconnects. That is [`send_frame`](Self::send_frame)'s
    /// return value, which is the only honest place for it — whether a peer is
    /// still there is knowable only by writing to it.
    fn reaches_a_peer(&self) -> bool {
        true
    }
}

/// An [`RpcEgress`] built from a plain closure — the shape every transport
/// in this workspace actually has (send into a channel, write a line to a
/// stream).
///
/// Free-standing rather than a blanket `impl RpcEgress for F` so a
/// transport that wants a *named* egress type with its own state can still
/// have one without coherence getting in the way.
pub struct FnEgress<F>(F);

impl<F> FnEgress<F>
where
    F: Fn(String) -> bool + Send + Sync + 'static,
{
    /// Wrap `sink` as this connection's egress, ready to share.
    ///
    /// Returns the shared trait object rather than `Self`: an egress is only
    /// ever held as `Arc<dyn RpcEgress>` (a frame carries one, a subscription
    /// clones one), so handing back the concrete type would put the same
    /// `Arc::new` at every call site.
    #[allow(
        clippy::new_ret_no_self,
        reason = "the only useful form of this value is the shared trait object; see above"
    )]
    pub fn new(sink: F) -> Arc<dyn RpcEgress> {
        Arc::new(Self(sink))
    }
}

impl<F> RpcEgress for FnEgress<F>
where
    F: Fn(String) -> bool + Send + Sync + 'static,
{
    fn send_frame(&self, frame: String) -> bool {
        (self.0)(frame)
    }
}

/// An egress with no connection behind it — for a producer that has a response
/// sink and nothing to speak to afterwards (a synthetic frame, a unit test
/// asserting only on the answer). Built by [`RpcFrame::answered_by`].
///
/// Both of its answers are `false`, and that is the honest pair: a frame
/// written here reached nobody, and nothing written here ever will.
/// `reaches_a_peer` being `false` is what makes `scene/subscribe` refuse such a
/// frame by name rather than register a stream that is silent forever.
pub struct NullEgress;

impl RpcEgress for NullEgress {
    fn send_frame(&self, _frame: String) -> bool {
        false
    }

    fn reaches_a_peer(&self) -> bool {
        false
    }
}

impl NullEgress {
    /// A shareable null egress.
    #[must_use]
    pub fn shared() -> Arc<dyn RpcEgress> {
        Arc::new(Self)
    }
}

/// R1552 §5.7 PINION-PR83 — where a frame came from: the connection it
/// arrived on and that connection's writer.
///
/// The pair every backend's dispatch entry threads through to
/// [`DispatchContext::with_frame_origin`](crate::DispatchContext::with_frame_origin).
/// Named because it travels as a unit through three signatures per backend,
/// and because `Option<(ConnId, &Arc<dyn RpcEgress>)>` spelled out costs four
/// lines of every one of them.
///
/// It is exactly the two [`RpcFrame`] fields a handler needs to keep writing
/// after the response; a handler that only answers ignores both.
pub type FrameOrigin<'a> = (ConnId, &'a Arc<dyn RpcEgress>);

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

    /// R1552 — the **one-shot response view of a connection's egress**: the
    /// reply a frame arriving on `egress` should be answered through.
    ///
    /// This is how a transport builds the pair, rather than building a
    /// reply and an egress separately from the same underlying writer. The
    /// difference is not style: it makes "a response and an unsolicited
    /// notification reach this client through the *same* writer, in the
    /// order they were produced" a structural property instead of a
    /// coincidence two constructions have to keep agreeing on. A
    /// subscription publishing between a request and its response is
    /// exactly when that ordering starts to matter.
    #[must_use]
    pub fn over(egress: &Arc<dyn RpcEgress>) -> Self {
        let egress = Arc::clone(egress);
        Self::new(move |response| {
            // A response to a client that has already gone is dropped, as
            // it was before this seam existed: `send` has no way to report
            // and no caller that could act on it.
            egress.send_frame(response);
        })
    }

    /// Deliver the one response for this frame. Consumes the reply so it
    /// can fire at most once.
    ///
    /// R1552 — "at most once" remains true and is now a statement about
    /// *this response*, not about the connection: an [`RpcEgress`] can write
    /// further frames to the same client afterwards, and a subscription
    /// does.
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
    /// R1552 PINION-PR83 — the **writer** of the connection this frame
    /// arrived on, for frames nobody asked for.
    ///
    /// Carried on the frame rather than looked up from a `conn -> egress`
    /// map because it is a fact about the frame's provenance: a frame
    /// arrived on a connection, and that connection can be written to. A
    /// map would be a second structure that has to be kept agreeing with
    /// [`RpcIngress::on_connect`] / [`on_disconnect`](RpcIngress::on_disconnect),
    /// with a window at each edge where the two disagree.
    ///
    /// A handler that wants to keep writing past this frame's response
    /// (`scene/subscribe`) clones it; every other handler ignores it.
    pub egress: Arc<dyn RpcEgress>,
}

impl RpcFrame {
    /// Pair a raw request envelope with the **egress of the connection it
    /// arrived on**, tagged with that connection's [`ConnId`]. The frame's
    /// [`reply`](Self::reply) is derived from that egress
    /// ([`RpcReply::over`]).
    ///
    /// R1552 — the reply is *derived* rather than passed in, so a transport
    /// cannot build a frame whose response goes somewhere other than where
    /// its notifications go. Before this seam the reply was the only
    /// egress, so there was nothing for it to disagree with.
    #[must_use]
    pub fn new(conn: ConnId, request: String, egress: Arc<dyn RpcEgress>) -> Self {
        let reply = RpcReply::over(&egress);
        Self {
            conn,
            request,
            reply,
            egress,
        }
    }

    /// Pair a request with an explicit reply sink and a [`NullEgress`] — a
    /// frame that can be *answered* but not spoken to afterwards.
    ///
    /// For a producer that has a response sink and no connection: a unit
    /// test asserting on the answer, or a synthetic frame. A
    /// `scene/subscribe` arriving this way is refused rather than silently
    /// registering a subscription whose notifications go nowhere — see
    /// [`mod@crate::subscribe`].
    #[must_use]
    pub fn answered_by(conn: ConnId, request: String, reply: RpcReply) -> Self {
        Self {
            conn,
            request,
            reply,
            egress: NullEgress::shared(),
        }
    }
}

impl fmt::Debug for RpcFrame {
    /// R1552 — hand-written because [`RpcEgress`] is a trait object and
    /// deriving would force every transport's egress to be [`Debug`] for a
    /// line nothing reads. The two fields that carry information (`conn`,
    /// `request`) print; the two sinks are placeholders, matching
    /// [`RpcReply`]'s own impl. `AppEvent` embeds this and derives `Debug`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcFrame")
            .field("conn", &self.conn)
            .field("request", &self.request)
            .field("reply", &self.reply)
            .field("egress", &"RpcEgress(..)")
            .finish()
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
