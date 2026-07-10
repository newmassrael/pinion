//! `scene/waitFor` async waiter registry — the §6.3 async-boundary landing
//! for change-driven waits (PR-50).
//!
//! The v0 [`wait_for`](mod@crate::wait_for) busy-polls a *constant* scene inside a
//! single synchronous dispatch: it can only report "matched on poll 1" or
//! "missed after N", never observe a change that arrives *between* polls,
//! because dispatch returns before any external state can land. Its own
//! docstring defers the real behaviour to "once the async boundary lands
//! (§6.3)". This registry is that landing.
//!
//! ## Model
//!
//! The registry owns a monotonic **change-generation counter** — its own
//! revision, distinct from the OCC [`SceneRevision`](pinion_core::SceneRevision)
//! that guards `scene/propose_change` (which external data arrival does *not*
//! bump — see the type's docs). A client issues `scene/waitFor { since: <n> }`
//! carrying the generation it last observed:
//!
//! * if the counter has already advanced past `since`, it answers immediately;
//! * otherwise the request's one-shot [`RpcReply`] is **parked** — the
//!   dispatch thread returns without blocking (a single-owner UI/dispatch loop
//!   must never park) — until the embedder [`notify_changed`](WaiterRegistry::notify_changed)s.
//!
//! External data arrival (a host pane's PTY output, a timer, a background
//! task) does **not** flow through `dispatch`, so nothing advances the counter
//! on its own; the embedder calls `notify_changed()` from wherever that
//! arrival is observed (e.g. a host's `on_dirty`, alongside its existing
//! repaint request — the same dirty signal, two consumers). That one call
//! bumps the counter and fires every parked reply the new generation surpasses.
//!
//! ## Why no new protocol
//!
//! [`RpcReply`] is already `FnOnce + Send` and every transport writer records
//! it *whenever* it fires (the socket writer runs on its own connection
//! thread). So a late reply reuses the existing one-shot request/response
//! path verbatim — no server-push, streaming, or subscription concept. The
//! reply simply fires off the dispatch thread, on the embedder's notify call.
//!
//! ## Threading
//!
//! `park` runs on the UI/dispatch thread; `notify_changed` may run on a
//! different thread (the arrival observer). The parked list is behind a
//! [`Mutex`] and the counter is an [`AtomicU64`]; a woken [`RpcReply`] is
//! `Send`, so firing it from the notify thread is sound. Replies are fired
//! **outside** the lock, so a reply sink that re-enters the registry cannot
//! deadlock.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;

use crate::dispatch::{JSONRPC_V2, Request, RequestId, Response, serialize};
use crate::transport::RpcReply;

/// One parked `scene/waitFor` reply, waiting for the change counter to
/// advance past [`since`](Self::since).
struct Parked {
    /// The generation the client last observed; the reply fires once the
    /// counter is strictly greater than this.
    since: u64,
    /// The originating request id, echoed in the response. `None` is a
    /// JSON-RPC notification (no id) — dropped without a response on wake,
    /// per the spec, rather than sent a reply nobody is awaiting.
    id: Option<RequestId>,
    /// The one-shot sink routing the response back to the originating
    /// transport, fired late on [`WaiterRegistry::notify_changed`].
    reply: RpcReply,
}

/// Registry of parked async `scene/waitFor` replies plus the monotonic
/// change-generation counter they wait on (PR-50 §6.3).
///
/// Embedder-owned and shared (an `Arc`) between the dispatch ingress — which
/// routes a `scene/waitFor` frame through [`try_async_wait_for`] to park or
/// answer it — and the arrival observer that calls
/// [`notify_changed`](Self::notify_changed) when the scene changes. See the
/// [module docs](self).
#[derive(Default)]
pub struct WaiterRegistry {
    /// Monotonic change generation. Bumped by
    /// [`notify_changed`](Self::notify_changed); read by
    /// [`current_revision`](Self::current_revision) and the async decision.
    revision: AtomicU64,
    parked: Mutex<Vec<Parked>>,
}

impl WaiterRegistry {
    /// A registry at generation `0` with no parked waiters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            revision: AtomicU64::new(0),
            parked: Mutex::new(Vec::new()),
        }
    }

    /// The current change generation. A client's first `scene/waitFor`
    /// typically passes `since: 0` and learns the live generation from the
    /// response; this accessor lets an embedder read it directly.
    #[must_use]
    pub fn current_revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Park a `scene/waitFor` reply until the change generation advances past
    /// `since`. The caller ([`try_async_wait_for`]) has already confirmed
    /// `current_revision() <= since`; a stale baseline answers immediately
    /// instead of parking.
    ///
    /// # Panics
    ///
    /// Panics only if the internal lock is poisoned (a prior holder panicked
    /// while mutating the list) — an unrecoverable invariant break, not a
    /// runtime condition a caller can provoke.
    pub fn park(&self, since: u64, id: Option<RequestId>, reply: RpcReply) {
        self.parked
            .lock()
            .expect("waiter registry lock poisoned")
            .push(Parked { since, id, reply });
    }

    /// Advance the change generation by one and wake every parked waiter the
    /// new generation surpasses (`since < new`), firing each reply with the
    /// [`waiter_response`] carrying the new generation. Returns the number
    /// woken.
    ///
    /// The embedder's single wake call — invoked from wherever an external
    /// scene change is observed (a host pane's `on_dirty`, a data tick),
    /// alongside its existing repaint request. Waiters whose `since >= new`
    /// stay parked (a bump they had already observed is not a change *they*
    /// were waiting for — impossible here since the counter is monotonic and
    /// they parked at `<= current`, but the boundary is enforced regardless).
    ///
    /// # Panics
    ///
    /// Panics only if the internal lock is poisoned (see [`park`](Self::park)).
    pub fn notify_changed(&self) -> usize {
        let new = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        // Drain the satisfied waiters out from under the lock, then fire their
        // replies with the lock released — a reply sink is opaque (it forwards
        // to a transport writer) and must never run while the registry mutex
        // is held.
        let woken: Vec<Parked> = {
            let mut parked = self.parked.lock().expect("waiter registry lock poisoned");
            let mut kept = Vec::with_capacity(parked.len());
            let mut fire = Vec::new();
            for w in parked.drain(..) {
                if w.since < new {
                    fire.push(w);
                } else {
                    kept.push(w);
                }
            }
            *parked = kept;
            fire
        };
        let count = woken.len();
        for w in woken {
            // A notification (no id) gets no response per JSON-RPC; its reply
            // is simply dropped (sending nothing).
            if let Some(id) = w.id {
                w.reply.send(waiter_response(Some(id), new));
            }
        }
        count
    }

    /// The number of currently parked waiters — for embedder introspection
    /// and tests.
    ///
    /// # Panics
    ///
    /// Panics only if the internal lock is poisoned (see [`park`](Self::park)).
    #[must_use]
    pub fn parked_count(&self) -> usize {
        self.parked
            .lock()
            .expect("waiter registry lock poisoned")
            .len()
    }
}

/// The shared **async `scene/waitFor` decision** — the SSOT both the GUI and
/// TUI ingresses route a frame through before normal dispatch, so the two
/// backends cannot drift on when a wait parks vs answers.
///
/// A `scene/waitFor { since: <n> }` is the async form (PR-50 §6.3): the client
/// passes the change generation it last observed and blocks until the scene
/// advances past it. This function claims exactly that shape:
///
/// * **Not an async waitFor** (a different method, or a `scene/waitFor` with
///   no numeric `since` — the v0 `{path, target, max_attempts}` busy-poll)
///   → returns `Err(reply)`, handing the reply back so the caller runs normal
///   [`dispatch`](crate::dispatch::dispatch) (the v0 path stays byte-unchanged
///   when no `since` is supplied).
/// * **Baseline already stale** (`registry.current_revision() > since`)
///   → answers immediately with [`waiter_response`] and returns `Ok(())`.
/// * **Baseline current** (`current <= since`) → [`park`](WaiterRegistry::park)s
///   the reply and returns `Ok(())`; the dispatch thread does not block, and
///   the embedder later [`notify_changed`](WaiterRegistry::notify_changed)s it.
///
/// The caller invokes this only when it has a registry wired (async
/// capability); an embedder with no registry never reaches here, so a
/// `since`-shaped request falls through to the v0 handler (which reports the
/// missing `{path, target}` params — async-unavailable surfaces as a param
/// error rather than a silent hang).
///
/// # Errors
///
/// `Err(reply)` when the frame is not an async waitFor — not a failure, the
/// signal to proceed with normal dispatch (the reply is returned unused).
pub fn try_async_wait_for(
    request: &Request,
    registry: &WaiterRegistry,
    reply: RpcReply,
) -> Result<(), RpcReply> {
    if request.method != "scene/waitFor" {
        return Err(reply);
    }
    let Some(since) = request
        .params
        .as_ref()
        .and_then(|p| p.get("since"))
        .and_then(serde_json::Value::as_u64)
    else {
        // No numeric `since` — the v0 synchronous busy-poll shape; hand the
        // reply back for normal dispatch.
        return Err(reply);
    };
    let current = registry.current_revision();
    if current > since {
        // Already advanced past the client's baseline — answer at once, with
        // the SAME wire shape a woken waiter gets.
        reply.send(waiter_response(request.id.clone(), current));
    } else {
        registry.park(since, request.id.clone(), reply);
    }
    Ok(())
}

/// Build the JSON-RPC 2.0 success response a satisfied `scene/waitFor`
/// returns, carrying the change `revision` the scene has advanced to. The
/// single SSOT for both the immediate-satisfaction path (baseline already
/// stale at dispatch) and the parked-then-woken path, so the wire shape is
/// identical however the wait resolved: `{ "changed": true, "revision": <n> }`.
#[must_use]
pub fn waiter_response(id: Option<RequestId>, revision: u64) -> String {
    // Serialized through the dispatch [`serialize`] SSOT (the one Response ->
    // String home, with its infallible-error fallback), so a woken wait's wire
    // frame is built exactly like every synchronous response.
    serialize(&Response {
        jsonrpc: JSONRPC_V2.to_string(),
        result: Some(json!({ "changed": true, "revision": revision })),
        error: None,
        id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    use serde_json::json;

    /// A recording reply sink: pushes whatever string it is sent into a
    /// shared vec, so a test can assert what (if anything) a parked reply
    /// fired.
    fn recording_reply(sink: &Arc<StdMutex<Vec<String>>>) -> RpcReply {
        let sink = Arc::clone(sink);
        RpcReply::new(move |s| sink.lock().unwrap().push(s))
    }

    /// A `scene/waitFor` request with the given `since` (or none for the v0
    /// busy-poll shape), id `1`.
    fn wait_for_request(since: Option<u64>) -> Request {
        Request {
            jsonrpc: "2.0".to_string(),
            method: "scene/waitFor".to_string(),
            params: since.map(|s| json!({ "since": s })),
            id: Some(RequestId::Num(1)),
        }
    }

    /// Park a reply at `since` directly (id `n`) with a recording sink.
    fn park_recording(reg: &WaiterRegistry, since: u64, n: i64, sink: &Arc<StdMutex<Vec<String>>>) {
        reg.park(since, Some(RequestId::Num(n)), recording_reply(sink));
    }

    #[test]
    fn waiter_response_is_jsonrpc_success_with_revision() {
        let s = waiter_response(Some(RequestId::Num(7)), 42);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert_eq!(v["result"]["changed"], true);
        assert_eq!(v["result"]["revision"], 42);
        assert!(v.get("error").is_none(), "success has no error member");
    }

    #[test]
    fn notify_advances_revision_and_wakes_surpassed_waiters() {
        let reg = WaiterRegistry::new();
        assert_eq!(reg.current_revision(), 0);
        let a = Arc::new(StdMutex::new(Vec::new())); // since 0
        let b = Arc::new(StdMutex::new(Vec::new())); // since 1
        park_recording(&reg, 0, 1, &a);
        park_recording(&reg, 1, 2, &b);
        assert_eq!(reg.parked_count(), 2);

        // First change → generation 1: surpasses since=0 (a), not since=1 (b).
        assert_eq!(reg.notify_changed(), 1, "only the since=0 waiter wakes");
        assert_eq!(reg.current_revision(), 1);
        assert_eq!(a.lock().unwrap().len(), 1, "since=0 reply fired");
        assert!(b.lock().unwrap().is_empty(), "since=1 reply still parked");
        assert_eq!(reg.parked_count(), 1);
        // The fired reply carries the NEW generation, not the baseline.
        let v: serde_json::Value = serde_json::from_str(&a.lock().unwrap()[0]).unwrap();
        assert_eq!(v["result"]["revision"], 1);
        assert_eq!(v["id"], 1);

        // Second change → generation 2 now surpasses since=1.
        assert_eq!(reg.notify_changed(), 1);
        assert_eq!(reg.current_revision(), 2);
        assert_eq!(b.lock().unwrap().len(), 1);
        assert_eq!(reg.parked_count(), 0);
    }

    #[test]
    fn one_notify_fires_every_eligible_waiter() {
        let reg = WaiterRegistry::new();
        let sinks: Vec<_> = (0..3)
            .map(|_| Arc::new(StdMutex::new(Vec::new())))
            .collect();
        for (i, s) in sinks.iter().enumerate() {
            let id = i64::try_from(i).unwrap();
            park_recording(&reg, 0, id, s);
        }
        // One change wakes all three (all parked at since=0 < 1).
        assert_eq!(
            reg.notify_changed(),
            3,
            "one change wakes every eligible waiter"
        );
        for s in &sinks {
            assert_eq!(s.lock().unwrap().len(), 1);
        }
        assert_eq!(reg.parked_count(), 0);
    }

    #[test]
    fn a_waiter_at_the_current_generation_needs_the_next_change() {
        // Park at since == current generation: the NEXT change surpasses it.
        let reg = WaiterRegistry::new();
        reg.notify_changed(); // generation 1, no waiters
        assert_eq!(reg.current_revision(), 1);
        let s = Arc::new(StdMutex::new(Vec::new()));
        park_recording(&reg, 1, 1, &s); // since == current
        assert_eq!(
            reg.notify_changed(),
            1,
            "the next change (gen 2) surpasses since=1"
        );
        assert_eq!(s.lock().unwrap().len(), 1);
    }

    #[test]
    fn notify_with_no_waiters_still_advances_the_generation() {
        let reg = WaiterRegistry::new();
        assert_eq!(reg.notify_changed(), 0, "no waiters woken");
        assert_eq!(reg.current_revision(), 1, "but the generation advanced");
    }

    #[test]
    fn notification_waiter_is_dropped_without_a_response() {
        // A parked waiter with no id (a JSON-RPC notification) is removed on
        // wake but sends nothing — a notification gets no reply per the spec.
        let reg = WaiterRegistry::new();
        let s = Arc::new(StdMutex::new(Vec::new()));
        reg.park(0, None, recording_reply(&s));
        assert_eq!(
            reg.notify_changed(),
            1,
            "the notification waiter is counted woken"
        );
        assert!(s.lock().unwrap().is_empty(), "but no response is sent");
        assert_eq!(reg.parked_count(), 0);
    }

    #[test]
    fn decision_hands_back_non_wait_for_and_sync_wait_for() {
        let reg = WaiterRegistry::new();
        // A different method → Err(reply), reply un-fired (normal dispatch runs).
        let s = Arc::new(StdMutex::new(Vec::new()));
        let other = Request {
            jsonrpc: "2.0".to_string(),
            method: "scene/query".to_string(),
            params: None,
            id: Some(RequestId::Num(1)),
        };
        assert!(
            try_async_wait_for(&other, &reg, recording_reply(&s)).is_err(),
            "a non-waitFor method is not claimed"
        );
        assert!(s.lock().unwrap().is_empty(), "reply handed back un-fired");
        // A waitFor with no `since` (the v0 busy-poll shape) → Err(reply) too.
        let s2 = Arc::new(StdMutex::new(Vec::new()));
        assert!(
            try_async_wait_for(&wait_for_request(None), &reg, recording_reply(&s2)).is_err(),
            "a since-less waitFor falls through to the v0 handler"
        );
        assert_eq!(
            reg.parked_count(),
            0,
            "nothing parked on the hand-back paths"
        );
    }

    #[test]
    fn decision_parks_when_baseline_is_current_then_notify_fires_it() {
        let reg = WaiterRegistry::new(); // current = 0
        let s = Arc::new(StdMutex::new(Vec::new()));
        // since = 0, current = 0 → not advanced → park (no immediate reply).
        assert!(try_async_wait_for(&wait_for_request(Some(0)), &reg, recording_reply(&s)).is_ok());
        assert_eq!(reg.parked_count(), 1, "current baseline parks");
        assert!(s.lock().unwrap().is_empty(), "parked, not answered yet");
        // The embedder observes an external change and notifies: the reply fires.
        assert_eq!(reg.notify_changed(), 1, "notify wakes the parked waiter");
        assert_eq!(s.lock().unwrap().len(), 1, "the woken reply fired");
        let v: serde_json::Value = serde_json::from_str(&s.lock().unwrap()[0]).unwrap();
        assert_eq!(v["result"]["revision"], 1);
    }

    #[test]
    fn decision_answers_immediately_when_baseline_is_stale() {
        let reg = WaiterRegistry::new();
        reg.notify_changed();
        reg.notify_changed(); // current = 2
        let s = Arc::new(StdMutex::new(Vec::new()));
        // since = 0, current = 2 → already advanced → immediate answer, no park.
        assert!(try_async_wait_for(&wait_for_request(Some(0)), &reg, recording_reply(&s)).is_ok());
        assert_eq!(reg.parked_count(), 0, "a stale baseline does not park");
        assert_eq!(s.lock().unwrap().len(), 1, "answered immediately");
        let v: serde_json::Value = serde_json::from_str(&s.lock().unwrap()[0]).unwrap();
        assert_eq!(v["result"]["revision"], 2, "carries the current generation");
    }
}
