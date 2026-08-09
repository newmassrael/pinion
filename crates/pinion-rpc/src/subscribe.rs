//! `scene/subscribe` — the server speaking first (R1552 §5.7 §6.3 §2 #2 §2 #7,
//! PINION-PR83).
//!
//! # The gap this closes
//!
//! Until R1552 a frame could be answered **at most once**: [`RpcReply`](crate::transport::RpcReply) is a
//! `FnOnce` and [`RpcFrame`](crate::transport::RpcFrame) held exactly one. So "one request, many answers" —
//! a change stream — was not merely unimplemented, it was *inexpressible* on
//! this transport at any price. A consumer wanting to follow the scene had to
//! re-issue a request per batch, paying a socket round trip each time.
//!
//! [`crate::waiter`]'s own module doc recorded the consequence — *"no
//! server-push, streaming, or subscription"* — as a property of the design it
//! was justifying. It was accurate, and it described an absence: `scene/waitFor`
//! parks ONE reply and fires it ONCE, so a client following the scene issues a
//! fresh wait per revision. That re-issue is correct (the revision cursor is a
//! half-open interval, so nothing is lost) and it costs a round trip per change.
//!
//! [`RpcEgress`] removes the impossibility; this module is the framework's own
//! consumer of it.
//!
//! # Wire form
//!
//! ```json
//! {"jsonrpc": "2.0", "id": 1, "method": "scene/subscribe", "params": {"since": 7}}
//! ```
//!
//! answers once with `{"subscription": 1, "revision": 7}`, and thereafter the
//! server writes, unprompted, one **notification** per advance:
//!
//! ```json
//! {"jsonrpc": "2.0", "method": "scene/changed",
//!  "params": {"subscription": 1, "revision": 8}}
//! ```
//!
//! `scene/unsubscribe {"subscription": 1}` ends it; so does the connection
//! closing, however it closes.
//!
//! # Why a notification, and not a second response
//!
//! PINION-PR83 asked first for `RpcReply::send_more` — the same frame answering
//! repeatedly. That is rejected on wire conformance, and the evidence is inside
//! this repository. JSON-RPC 2.0 section 5 pairs **one** Response to one Request, and
//! every client built on that pairing keys a pending map by `id` and *removes*
//! the entry when the first answer lands. `tools/rpc_verify.py` is such a
//! client: its `request` loop reads until `msg.get("id") == request_id` and
//! **discards** every frame that does not match. A second Response carrying the
//! same `id` is therefore not merely irregular — it is unreadable by the
//! project's own harness and by every conforming library.
//!
//! A notification (a `method`, no `id` — JSON-RPC 2.0, section 4.1) is the one form a client can
//! tell apart from its own answer. That discriminability is the whole point: the
//! stream shares a channel with request/response, so it has to be separable from
//! it. This is also what LSP (`$/progress`), DAP, the Chrome `DevTools` Protocol
//! and Ethereum's `eth_subscription` all do.
//!
//! # What a subscriber is promised
//!
//! **The revision cursor, coalesced.** A subscription holds the revision it last
//! delivered and is sent one notification per *advance past it*, carrying the
//! revision reached — not one per bump. Two bumps between publishes produce one
//! notification naming the later revision. That is the same contract
//! `scene/waitFor` has, for the same reason ([`pinion_core::SceneRevision`] is
//! one token for one scene), and it is the honest one: the revision is a
//! generation counter, not an event log, so a subscriber learns *that the scene
//! moved and to where*, then reads what it needs. A subscriber that must not
//! miss intermediate states is asking for an event log, which this is not.
//!
//! **No gap at either edge.** `since` is the revision the client last observed,
//! exactly as `scene/waitFor` takes it, so a subscription opened after a read is
//! caught up on anything that landed in between. A reconnecting client passes
//! the last revision it saw and resumes precisely there.
//!
//! **Never a notification for a subscription it has not been told about.** A
//! subscription is registered *disarmed* and armed by the dispatch site after
//! the subscribing frame's response has gone out ([`SubscriptionRegistry::arm_pending`]).
//! Without that, a bump landing between the register and the reply would write
//! `scene/changed {"subscription": 4}` to a client that has not yet learned `4`.
//! The window is sub-microsecond and the fix is structural rather than a rule to
//! remember — the same trade PR-48's bind-time exposure made.
//!
//! # Ordering
//!
//! A response and a notification to the same client are written through the
//! **same** [`RpcEgress`], because [`RpcReply::over`](crate::transport::RpcReply::over) derives the reply from it.
//! So the two cannot interleave mid-frame and cannot be reordered against each
//! other; a transport that built the two paths separately would have to keep
//! that agreement by hand.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::json;

use crate::transport::{ConnId, RpcEgress};

/// The method name of the notification a subscription delivers.
///
/// Public because it is a wire constant a consumer matches on, and because the
/// demo asserts against this rather than a re-typed literal.
pub const CHANGED_METHOD: &str = "scene/changed";

/// Typed errors the subscribe/unsubscribe dispatchers return. The variant name
/// rides in `error.data` so an agent pattern-matches rather than parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscribeError {
    /// The frame arrived on a transport that cannot be written to unprompted,
    /// so a subscription's notifications would go nowhere.
    ///
    /// Refused at subscribe time rather than registered and silently discarded:
    /// a client that believes it is subscribed and never hears anything cannot
    /// tell that from a scene that never changed.
    NotStreamable,
    /// The embedder installed no subscription registry on the dispatch context
    /// — a backend with no connection-bound transport at all.
    SubscriptionsUnavailable,
    /// `params.since` was present but not a non-negative integer.
    InvalidSince,
    /// `params.subscription` was missing or not a non-negative integer.
    InvalidSubscriptionId,
    /// No live subscription carries that id — already closed, never opened, or
    /// belonging to a different connection (see [`SubscriptionRegistry::close`]).
    UnknownSubscription,
}

impl SubscribeError {
    /// The stable `error.data` discriminant.
    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            Self::NotStreamable => "not_streamable",
            Self::SubscriptionsUnavailable => "subscriptions_unavailable",
            Self::InvalidSince => "invalid_since",
            Self::InvalidSubscriptionId => "invalid_subscription_id",
            Self::UnknownSubscription => "unknown_subscription",
        }
    }

    /// The human-facing message.
    #[must_use]
    pub fn message(&self) -> &'static str {
        match self {
            Self::NotStreamable => {
                "this transport cannot deliver unsolicited frames; a subscription would be silent"
            }
            Self::SubscriptionsUnavailable => "this backend has no subscription registry",
            Self::InvalidSince => "params.since must be a non-negative integer",
            Self::InvalidSubscriptionId => "params.subscription must be a non-negative integer",
            Self::UnknownSubscription => "no live subscription carries that id",
        }
    }
}

/// What `scene/subscribe` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubscribeOutcome {
    /// The opaque, process-unique id every notification for this stream
    /// carries, and the id `scene/unsubscribe` takes.
    pub subscription: u64,
    /// The revision this subscription is caught up to at the moment it was
    /// opened — the `since` it was given. A notification arrives when the scene
    /// passes it.
    pub revision: u64,
}

/// What `scene/unsubscribe` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnsubscribeOutcome {
    /// The id that was closed.
    pub subscription: u64,
    /// How many notifications this subscription delivered before it closed —
    /// so a client can reconcile its own count against the server's without a
    /// second method.
    pub delivered_count: u64,
}

/// One live subscription, as `scene/subscriptions` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubscriptionView {
    /// The subscription's opaque id.
    pub subscription: u64,
    /// The connection it will be delivered on ([`ConnId::get`]).
    pub conn: u64,
    /// The revision it has been brought up to.
    pub revision: u64,
    /// How many notifications it has delivered.
    pub delivered_count: u64,
    /// Whether it is armed — `false` only between its registration and its own
    /// response going out, which no client can observe for its OWN
    /// subscription (the answer carrying the id has not been written yet).
    pub armed: bool,
}

/// What `scene/subscriptions` answers with — the §2 #7 read side.
///
/// The toolkit publishes no equivalent for local server: a toolkit application
/// cannot enumerate who is listening to what, because nothing in the toolkit
/// binds a server-initiated write to a named stream in the first place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubscriptionsOutcome {
    /// Every live subscription, in id order.
    pub subscriptions: Vec<SubscriptionView>,
    /// Total notifications this registry has written since the process began,
    /// including those for subscriptions since closed. A monotonic counter, so
    /// a test can assert "nothing was published" as a value rather than as the
    /// absence of an observation.
    pub published_total: u64,
}

/// One registered stream.
struct Subscription {
    id: u64,
    conn: ConnId,
    /// The revision this subscription has been brought up to. Advanced on every
    /// notification, so the next publish knows whether it owes one.
    delivered: u64,
    delivered_count: u64,
    /// `false` between registration and the subscribing frame's response going
    /// out — see the module docs.
    armed: bool,
    egress: Arc<dyn RpcEgress>,
}

/// What one [`SubscriptionRegistry::publish`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PublishReport {
    /// Notifications written.
    pub sent: usize,
    /// Subscriptions dropped because their egress reported the peer gone.
    pub pruned: usize,
}

/// The live subscriptions, embedder-owned and shared between the dispatch site
/// (which opens and closes them) and the [`pinion_core::SceneRevision`] observer
/// (which publishes).
///
/// Holds no revision counter of its own, for the reason [`crate::waiter`] gives:
/// one scene has one version, and a private counter forks that namespace.
#[derive(Default)]
pub struct SubscriptionRegistry {
    subs: Mutex<Vec<Subscription>>,
    next_id: AtomicU64,
    published_total: AtomicU64,
}

impl SubscriptionRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a stream for `conn`, caught up to `since`, and return its id.
    ///
    /// Registered **disarmed**: [`arm_pending`](Self::arm_pending) makes it
    /// eligible for delivery, and the dispatch site calls that only after this
    /// frame's response has gone out. See the module docs.
    ///
    /// # Panics
    /// Only if the internal lock is poisoned (a prior holder panicked while
    /// mutating the list) — an unrecoverable invariant break.
    pub fn open(&self, conn: ConnId, egress: &Arc<dyn RpcEgress>, since: u64) -> u64 {
        // Ids start at 1 so a bare `0` is never a live subscription, matching
        // `ConnId`'s own reservation.
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        self.subs
            .lock()
            .expect("subscription registry lock poisoned")
            .push(Subscription {
                id,
                conn,
                delivered: since,
                delivered_count: 0,
                armed: false,
                egress: Arc::clone(egress),
            });
        id
    }

    /// Arm every subscription registered but not yet delivered-to, and report
    /// how many were armed.
    ///
    /// Called by the dispatch site after the response has been written. Arming
    /// *all* pending is correct because dispatch is serialized on one thread:
    /// at most one subscribe can be un-armed at this point, the one this frame
    /// opened.
    ///
    /// # Panics
    /// Only if the internal lock is poisoned (see [`open`](Self::open)).
    pub fn arm_pending(&self) -> usize {
        let mut subs = self
            .subs
            .lock()
            .expect("subscription registry lock poisoned");
        let mut armed = 0;
        for s in subs.iter_mut() {
            if !s.armed {
                s.armed = true;
                armed += 1;
            }
        }
        armed
    }

    /// Close the subscription `id` **owned by `conn`**, returning how many
    /// notifications it delivered.
    ///
    /// Scoped to the connection deliberately: ids are process-unique but
    /// guessable (they are small integers), and one client closing another
    /// client's stream is a cross-connection effect no method on this surface
    /// should have. A mismatched owner reads as [`SubscribeError::UnknownSubscription`]
    /// — the same answer as a stale id, because to the asking connection those
    /// two are the same fact.
    ///
    /// # Panics
    /// Only if the internal lock is poisoned (see [`open`](Self::open)).
    pub fn close(&self, conn: ConnId, id: u64) -> Option<u64> {
        let mut subs = self
            .subs
            .lock()
            .expect("subscription registry lock poisoned");
        let at = subs.iter().position(|s| s.id == id && s.conn == conn)?;
        Some(subs.remove(at).delivered_count)
    }

    /// Drop every subscription belonging to `conn`, returning how many.
    ///
    /// The crash-safe half: the ingress calls this from
    /// [`crate::RpcIngress::on_disconnect`], which fires however the client went
    /// away, so a stream cannot outlive the connection it writes to even when no
    /// `scene/unsubscribe` ever arrives.
    ///
    /// # Panics
    /// Only if the internal lock is poisoned (see [`open`](Self::open)).
    pub fn close_connection(&self, conn: ConnId) -> usize {
        let mut subs = self
            .subs
            .lock()
            .expect("subscription registry lock poisoned");
        let before = subs.len();
        subs.retain(|s| s.conn != conn);
        before - subs.len()
    }

    /// Deliver one `scene/changed` notification to every armed subscription the
    /// scene has advanced past, and drop the ones whose peer is gone.
    ///
    /// Called from the [`pinion_core::SceneRevision`] observer with the
    /// just-bumped value, and once by the dispatch site after a subscribe so a
    /// stale `since` is caught up immediately.
    ///
    /// The frames are built under the lock and **written outside it**: an egress
    /// is opaque, and running one under the registry mutex would let a slow or
    /// re-entrant transport stall every other subscriber. Same discipline
    /// [`crate::waiter::WaiterRegistry::wake`] uses for reply sinks.
    ///
    /// # Panics
    /// Only if the internal lock is poisoned (see [`open`](Self::open)).
    pub fn publish(&self, revision: u64) -> PublishReport {
        // Under the lock: pick the owed frames and advance the cursors. The
        // cursor moves here rather than after the write, so a concurrent second
        // publish at the same revision cannot double-send.
        let owed: Vec<(usize, u64, String, Arc<dyn RpcEgress>)> = {
            let mut subs = self
                .subs
                .lock()
                .expect("subscription registry lock poisoned");
            let mut owed = Vec::new();
            for (index, s) in subs.iter_mut().enumerate() {
                if s.armed && s.delivered < revision {
                    s.delivered = revision;
                    s.delivered_count += 1;
                    owed.push((
                        index,
                        s.id,
                        changed_notification(s.id, revision),
                        Arc::clone(&s.egress),
                    ));
                }
            }
            owed
        };

        let mut report = PublishReport::default();
        let mut gone: Vec<u64> = Vec::new();
        for (_index, id, frame, egress) in owed {
            if egress.send_frame(frame) {
                report.sent += 1;
            } else {
                gone.push(id);
            }
        }
        self.published_total
            .fetch_add(report.sent as u64, Ordering::Relaxed);

        if !gone.is_empty() {
            let mut subs = self
                .subs
                .lock()
                .expect("subscription registry lock poisoned");
            let before = subs.len();
            subs.retain(|s| !gone.contains(&s.id));
            report.pruned = before - subs.len();
        }
        report
    }

    /// How many subscriptions are live.
    ///
    /// # Panics
    /// Only if the internal lock is poisoned (see [`open`](Self::open)).
    #[must_use]
    pub fn count(&self) -> usize {
        self.subs
            .lock()
            .expect("subscription registry lock poisoned")
            .len()
    }

    /// Every live subscription, in id order — the §2 #7 read side.
    ///
    /// # Panics
    /// Only if the internal lock is poisoned (see [`open`](Self::open)).
    #[must_use]
    pub fn views(&self) -> SubscriptionsOutcome {
        let subs = self
            .subs
            .lock()
            .expect("subscription registry lock poisoned");
        let mut subscriptions: Vec<SubscriptionView> = subs
            .iter()
            .map(|s| SubscriptionView {
                subscription: s.id,
                conn: s.conn.get(),
                revision: s.delivered,
                delivered_count: s.delivered_count,
                armed: s.armed,
            })
            .collect();
        subscriptions.sort_by_key(|v| v.subscription);
        SubscriptionsOutcome {
            subscriptions,
            published_total: self.published_total.load(Ordering::Relaxed),
        }
    }
}

/// The **process's** subscription registry — the one the built-in transports
/// and both backends share.
///
/// A process-level singleton rather than an [`pinion_core::Owner`]-scoped
/// [`pinion_core::ProviderSlot`] (which is where [`crate::WaiterRegistry`]
/// lives), for two structural reasons rather than convenience:
///
/// 1. **A subscription is keyed by [`ConnId`], which is process-unique by
///    construction** — one counter, spanning every transport in the process.
///    So the set of live subscriptions is a process-level fact; there is no
///    per-window or per-binding reading of "who is subscribed".
/// 2. **The cleanup path has no `Owner` to resolve from.**
///    [`crate::RpcIngress::on_disconnect`] runs on a transport's reader thread,
///    where the owner thread-local is empty. An owner-scoped slot could not
///    serve that call at all — and it is the call that keeps a stream from
///    outliving its connection.
///
/// A consumer wanting an isolated registry constructs [`SubscriptionRegistry`]
/// directly; this is the shared default, not the only one.
pub fn process_registry() -> &'static SubscriptionRegistry {
    static REGISTRY: std::sync::OnceLock<SubscriptionRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(SubscriptionRegistry::new)
}

/// Build the `scene/changed` notification frame for one subscription.
///
/// A JSON-RPC 2.0 **notification**: `method` + `params`, and no `id` — see the
/// module docs for why the stream cannot be responses. Built here rather than at
/// the call site so the one wire form has one home.
#[must_use]
pub fn changed_notification(subscription: u64, revision: u64) -> String {
    // `to_string` on a `serde_json::Value` cannot fail for a map of numbers.
    json!({
        "jsonrpc": "2.0",
        "method": CHANGED_METHOD,
        "params": { "subscription": subscription, "revision": revision },
    })
    .to_string()
}

/// The connection a frame arrived on, paired with the registry its
/// subscriptions live in — everything `scene/subscribe` needs that a bare
/// [`crate::Request`] does not carry.
///
/// Threaded on [`crate::DispatchContext`] like the other embedder-supplied
/// hooks: absent (`None`) on a backend with no connection-bound transport, in
/// which case the three methods answer
/// [`SubscribeError::SubscriptionsUnavailable`] rather than being missing from
/// the surface — an agent learns the method exists and why it cannot serve.
pub struct Subscriber<'a> {
    /// The connection this frame arrived on.
    pub conn: ConnId,
    /// That connection's writer.
    pub egress: &'a Arc<dyn RpcEgress>,
    /// Where its subscriptions live.
    pub registry: &'a SubscriptionRegistry,
}

/// `scene/subscribe` — open a change stream on this frame's connection.
///
/// `params.since` defaults to `current` (subscribe to what happens *next*);
/// passing the revision last observed closes the gap between a read and the
/// subscribe.
///
/// # Errors
/// [`SubscribeError::SubscriptionsUnavailable`] when the embedder threaded no
/// [`Subscriber`]; [`SubscribeError::NotStreamable`] when the frame's transport
/// cannot write unprompted; [`SubscribeError::InvalidSince`] for a malformed
/// `since`.
pub fn subscribe(
    subscriber: Option<&Subscriber<'_>>,
    params: Option<&serde_json::Value>,
    current: u64,
) -> Result<SubscribeOutcome, SubscribeError> {
    let subscriber = subscriber.ok_or(SubscribeError::SubscriptionsUnavailable)?;
    if !subscriber.egress.reaches_a_peer() {
        return Err(SubscribeError::NotStreamable);
    }
    let since = match params.and_then(|p| p.get("since")) {
        None | Some(serde_json::Value::Null) => current,
        Some(v) => v.as_u64().ok_or(SubscribeError::InvalidSince)?,
    };
    let id = subscriber
        .registry
        .open(subscriber.conn, subscriber.egress, since);
    Ok(SubscribeOutcome {
        subscription: id,
        revision: since,
    })
}

/// `scene/unsubscribe` — end a stream this connection opened.
///
/// # Errors
/// [`SubscribeError::SubscriptionsUnavailable`] when no [`Subscriber`] is
/// threaded; [`SubscribeError::InvalidSubscriptionId`] for a malformed id;
/// [`SubscribeError::UnknownSubscription`] when this connection owns no live
/// subscription with that id.
pub fn unsubscribe(
    subscriber: Option<&Subscriber<'_>>,
    params: Option<&serde_json::Value>,
) -> Result<UnsubscribeOutcome, SubscribeError> {
    let subscriber = subscriber.ok_or(SubscribeError::SubscriptionsUnavailable)?;
    let id = params
        .and_then(|p| p.get("subscription"))
        .and_then(serde_json::Value::as_u64)
        .ok_or(SubscribeError::InvalidSubscriptionId)?;
    let delivered_count = subscriber
        .registry
        .close(subscriber.conn, id)
        .ok_or(SubscribeError::UnknownSubscription)?;
    Ok(UnsubscribeOutcome {
        subscription: id,
        delivered_count,
    })
}

/// `scene/subscriptions` — enumerate the live streams (§2 #7).
///
/// Answers for the whole registry, not just this connection: it is a diagnostic
/// read, and "who is listening to this app" is the question an agent driving it
/// actually has. Ids are opaque, so this discloses no client state.
///
/// # Errors
/// [`SubscribeError::SubscriptionsUnavailable`] when no [`Subscriber`] is
/// threaded.
pub fn subscriptions(
    subscriber: Option<&Subscriber<'_>>,
) -> Result<SubscriptionsOutcome, SubscribeError> {
    let subscriber = subscriber.ok_or(SubscribeError::SubscriptionsUnavailable)?;
    Ok(subscriber.registry.views())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    use super::{
        CHANGED_METHOD, SubscribeError, Subscriber, SubscriptionRegistry, changed_notification,
        subscribe, subscriptions, unsubscribe,
    };
    use crate::transport::{ConnId, FnEgress, NullEgress, RpcEgress};

    /// A recording egress: keeps every frame written to it, and can be told to
    /// report the peer gone.
    fn recording(
        sink: &Arc<StdMutex<Vec<String>>>,
        alive: &Arc<std::sync::atomic::AtomicBool>,
    ) -> Arc<dyn RpcEgress> {
        let sink = Arc::clone(sink);
        let alive = Arc::clone(alive);
        FnEgress::new(move |frame: String| {
            if !alive.load(std::sync::atomic::Ordering::Relaxed) {
                return false;
            }
            sink.lock().unwrap().push(frame);
            true
        })
    }

    /// A live recording egress: its captured frames, the flag that kills its
    /// peer, and the egress itself.
    type Recorder = (
        Arc<StdMutex<Vec<String>>>,
        Arc<std::sync::atomic::AtomicBool>,
        Arc<dyn RpcEgress>,
    );

    fn live() -> Recorder {
        let sink = Arc::new(StdMutex::new(Vec::new()));
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let egress = recording(&sink, &alive);
        (sink, alive, egress)
    }

    #[test]
    fn the_notification_is_a_jsonrpc_notification_not_a_response() {
        // The property the whole design turns on: no `id`, so a client keyed on
        // its own pending ids can tell this from its answer. A `result` member
        // would make it a (malformed) response.
        let v: serde_json::Value =
            serde_json::from_str(&changed_notification(3, 42)).expect("valid JSON");
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["method"], CHANGED_METHOD);
        assert_eq!(v["params"]["subscription"], 3);
        assert_eq!(v["params"]["revision"], 42);
        assert!(v.get("id").is_none(), "a notification carries no id");
        assert!(v.get("result").is_none(), "not a response");
        assert!(v.get("error").is_none(), "not an error");
    }

    #[test]
    fn an_armed_subscription_is_published_to_once_per_advance() {
        let registry = SubscriptionRegistry::new();
        let (sink, _alive, egress) = live();
        let conn = ConnId::allocate();
        let id = registry.open(conn, &egress, 0);
        registry.arm_pending();

        assert_eq!(registry.publish(1).sent, 1);
        assert_eq!(registry.publish(1).sent, 0, "same revision owes nothing");
        assert_eq!(registry.publish(2).sent, 1);

        let frames = sink.lock().unwrap().clone();
        assert_eq!(frames.len(), 2);
        for (frame, expected) in frames.iter().zip([1u64, 2]) {
            let v: serde_json::Value = serde_json::from_str(frame).unwrap();
            assert_eq!(v["params"]["subscription"], id);
            assert_eq!(v["params"]["revision"], expected);
        }
    }

    #[test]
    fn two_bumps_between_publishes_coalesce_into_one_notification() {
        // The stated contract: a subscriber learns THAT the scene moved and to
        // WHERE, not every intermediate generation.
        let registry = SubscriptionRegistry::new();
        let (sink, _alive, egress) = live();
        registry.open(ConnId::allocate(), &egress, 0);
        registry.arm_pending();
        assert_eq!(registry.publish(5).sent, 1, "one notification for 0 -> 5");
        let v: serde_json::Value = serde_json::from_str(&sink.lock().unwrap()[0]).unwrap();
        assert_eq!(v["params"]["revision"], 5, "naming the revision reached");
    }

    #[test]
    fn an_unarmed_subscription_is_never_published_to() {
        // The window PR-83's shape would have left open: a bump between the
        // register and the reply would name an id the client has not learned.
        let registry = SubscriptionRegistry::new();
        let (sink, _alive, egress) = live();
        registry.open(ConnId::allocate(), &egress, 0);
        assert_eq!(registry.publish(9).sent, 0, "not armed, not delivered");
        assert!(sink.lock().unwrap().is_empty());
        assert_eq!(registry.arm_pending(), 1);
        assert_eq!(
            registry.publish(9).sent,
            1,
            "the advance is still owed once armed"
        );
    }

    #[test]
    fn a_stale_since_is_owed_a_notification_immediately() {
        // The no-gap edge: a client that read at revision 2 and subscribed at
        // `since: 2` while the scene had reached 4 must not miss 3-4.
        let registry = SubscriptionRegistry::new();
        let (sink, _alive, egress) = live();
        registry.open(ConnId::allocate(), &egress, 2);
        registry.arm_pending();
        assert_eq!(registry.publish(4).sent, 1);
        let v: serde_json::Value = serde_json::from_str(&sink.lock().unwrap()[0]).unwrap();
        assert_eq!(v["params"]["revision"], 4);
    }

    #[test]
    fn a_dead_peer_prunes_its_subscription() {
        let registry = SubscriptionRegistry::new();
        let (_sink, alive, egress) = live();
        registry.open(ConnId::allocate(), &egress, 0);
        registry.arm_pending();
        alive.store(false, std::sync::atomic::Ordering::Relaxed);
        let report = registry.publish(1);
        assert_eq!(report.sent, 0);
        assert_eq!(report.pruned, 1);
        assert_eq!(registry.count(), 0, "a stream to nobody does not persist");
    }

    #[test]
    fn a_disconnect_closes_only_that_connections_streams() {
        let registry = SubscriptionRegistry::new();
        let (_sa, _aa, ea) = live();
        let (_sb, _ab, eb) = live();
        let a = ConnId::allocate();
        let b = ConnId::allocate();
        registry.open(a, &ea, 0);
        registry.open(a, &ea, 0);
        let kept = registry.open(b, &eb, 0);
        assert_eq!(registry.close_connection(a), 2);
        let views = registry.views();
        assert_eq!(views.subscriptions.len(), 1);
        assert_eq!(views.subscriptions[0].subscription, kept, "attribution");
    }

    #[test]
    fn one_connection_cannot_close_anothers_subscription() {
        let registry = SubscriptionRegistry::new();
        let (_s, _a, egress) = live();
        let owner = ConnId::allocate();
        let other = ConnId::allocate();
        let id = registry.open(owner, &egress, 0);
        assert_eq!(registry.close(other, id), None, "not this connection's");
        assert_eq!(registry.count(), 1, "and it is still live");
        assert_eq!(registry.close(owner, id), Some(0));
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn a_frame_that_cannot_be_written_to_is_refused_a_subscription() {
        // `NullEgress` is what a synthetic frame carries. Registering a stream
        // on it would leave a client believing it is subscribed while hearing
        // nothing — indistinguishable from a scene that never changes.
        let registry = SubscriptionRegistry::new();
        let egress = NullEgress::shared();
        let subscriber = Subscriber {
            conn: ConnId::allocate(),
            egress: &egress,
            registry: &registry,
        };
        assert_eq!(
            subscribe(Some(&subscriber), None, 0),
            Err(SubscribeError::NotStreamable)
        );
        assert_eq!(registry.count(), 0, "nothing registered");
    }

    #[test]
    fn an_absent_registry_names_itself_rather_than_the_method_vanishing() {
        assert_eq!(
            subscribe(None, None, 0),
            Err(SubscribeError::SubscriptionsUnavailable)
        );
        assert_eq!(
            unsubscribe(None, None),
            Err(SubscribeError::SubscriptionsUnavailable)
        );
        assert!(subscriptions(None).is_err());
    }

    #[test]
    fn subscribe_defaults_since_to_the_current_revision() {
        let registry = SubscriptionRegistry::new();
        let (_s, _a, egress) = live();
        let subscriber = Subscriber {
            conn: ConnId::allocate(),
            egress: &egress,
            registry: &registry,
        };
        let out = subscribe(Some(&subscriber), None, 7).expect("opens");
        assert_eq!(out.revision, 7, "subscribed to what happens next");
        // And an explicit `since` overrides it.
        let out2 =
            subscribe(Some(&subscriber), Some(&serde_json::json!({"since": 3})), 7).expect("opens");
        assert_eq!(out2.revision, 3);
        assert_ne!(out.subscription, out2.subscription, "ids are distinct");
    }

    #[test]
    fn a_malformed_since_is_named_rather_than_silently_defaulted() {
        let registry = SubscriptionRegistry::new();
        let (_s, _a, egress) = live();
        let subscriber = Subscriber {
            conn: ConnId::allocate(),
            egress: &egress,
            registry: &registry,
        };
        assert_eq!(
            subscribe(
                Some(&subscriber),
                Some(&serde_json::json!({"since": "soon"})),
                0
            ),
            Err(SubscribeError::InvalidSince)
        );
        assert_eq!(
            subscribe(
                Some(&subscriber),
                Some(&serde_json::json!({"since": -1})),
                0
            ),
            Err(SubscribeError::InvalidSince),
            "a negative revision is not a revision"
        );
    }

    #[test]
    fn unsubscribe_reports_what_the_stream_delivered() {
        let registry = SubscriptionRegistry::new();
        let (_s, _a, egress) = live();
        let conn = ConnId::allocate();
        let subscriber = Subscriber {
            conn,
            egress: &egress,
            registry: &registry,
        };
        let out = subscribe(Some(&subscriber), None, 0).expect("opens");
        registry.arm_pending();
        registry.publish(1);
        registry.publish(2);
        let closed = unsubscribe(
            Some(&subscriber),
            Some(&serde_json::json!({"subscription": out.subscription})),
        )
        .expect("closes");
        assert_eq!(closed.delivered_count, 2);
        assert_eq!(
            unsubscribe(
                Some(&subscriber),
                Some(&serde_json::json!({"subscription": out.subscription}))
            ),
            Err(SubscribeError::UnknownSubscription),
            "closing twice is not silently fine"
        );
    }

    #[test]
    fn the_registry_publishes_a_running_total_across_closed_streams() {
        let registry = SubscriptionRegistry::new();
        let (_s, _a, egress) = live();
        let conn = ConnId::allocate();
        let id = registry.open(conn, &egress, 0);
        registry.arm_pending();
        registry.publish(1);
        assert_eq!(registry.views().published_total, 1);
        registry.close(conn, id);
        assert_eq!(
            registry.views().published_total,
            1,
            "the total survives the stream that earned it"
        );
        assert!(registry.views().subscriptions.is_empty());
    }
}
