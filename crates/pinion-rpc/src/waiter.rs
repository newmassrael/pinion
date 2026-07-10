//! `scene/waitFor` async waiter registry — the §6.3 async-boundary landing
//! for change-driven waits (PR-50, redesigned R1270).
//!
//! The v0 [`wait_for`](mod@crate::wait_for) busy-polls a *constant* scene inside
//! a single synchronous dispatch: it can only report "matched on poll 1" or
//! "missed after N", never observe a change that arrives *between* polls,
//! because dispatch returns before any external state can land. Its own
//! docstring defers the real behaviour to "once the async boundary lands
//! (§6.3)". This registry is that landing.
//!
//! ## One token (R1270)
//!
//! A client issues `scene/waitFor { since: <revision> }` carrying the
//! [`SceneRevision`] it last observed. The registry does **not** own a counter:
//! it parks replies keyed by `since` and is woken off the **one** scene version
//! token — the same OCC [`SceneRevision`] that
//! guards `scene/propose_change`, which the embedder installs a
//! [wake observer](pinion_core::SceneRevision::set_observer) on so that
//! **every** bump — a dispatched mutation, shell input, or an external-data
//! arrival — advances that token and [`wake`](WaiterRegistry::wake)s parked
//! waiters. (An earlier revision of this module minted a *private* counter;
//! that forked the scene-version namespace and left the OCC token stale on
//! external arrival — one scene must have one version.)
//!
//! ## No lost wakeup (the condvar discipline)
//!
//! [`park_if_current`](WaiterRegistry::park_if_current) reads the revision
//! **under the parked-list lock** and, atomically under that same lock, either
//! answers immediately (`current > since`) or parks. [`wake`](WaiterRegistry::wake)
//! is called *after* the counter is bumped and takes the same lock to drain.
//! So the predicate check and the enqueue are atomic with respect to the
//! notifier's bump+drain — a bump can never slip between "decide to park" and
//! "parked", the lost-wakeup the naive check-then-park has.
//!
//! ## Why no new protocol
//!
//! [`RpcReply`] is already `FnOnce + Send` and every transport writer records
//! it *whenever* it fires. So a late reply reuses the existing one-shot
//! request/response path verbatim — no server-push, streaming, or subscription.
//! The reply fires off the dispatch thread, on the bump that woke it.

use std::ops::ControlFlow;
use std::sync::Mutex;

use pinion_core::SceneRevision;
use serde_json::json;

use crate::dispatch::{JSONRPC_V2, Request, RequestId, Response, serialize};
use crate::transport::RpcReply;

/// One parked `scene/waitFor` reply, waiting for the scene revision to advance
/// past [`since`](Self::since).
struct Parked {
    /// The revision the client last observed; the reply fires once the scene
    /// revision is strictly greater than this.
    since: u64,
    /// The originating request id, echoed in the response. `None` is a
    /// JSON-RPC notification (no id) — dropped without a response on wake,
    /// per the spec, rather than sent a reply nobody is awaiting.
    id: Option<RequestId>,
    /// The one-shot sink routing the response back to the originating
    /// transport, fired late on [`WaiterRegistry::wake`].
    reply: RpcReply,
}

/// Registry of parked async `scene/waitFor` replies (PR-50 §6.3, R1270).
///
/// Embedder-owned and shared (an `Arc`) between the dispatch ingress — which
/// routes a `scene/waitFor` frame through [`try_async_wait_for`] to park or
/// answer it — and the [`SceneRevision`] wake observer the embedder installs
/// (`move |new| registry.wake(new)`), which fires on every scene bump. The
/// registry holds no version counter of its own; the [`SceneRevision`] is the
/// single source of truth for "what generation is the scene at". See the
/// [module docs](self).
#[derive(Default)]
pub struct WaiterRegistry {
    parked: Mutex<Vec<Parked>>,
}

impl WaiterRegistry {
    /// A registry with no parked waiters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            parked: Mutex::new(Vec::new()),
        }
    }

    /// Atomically decide, **under the parked-list lock**, whether to answer a
    /// `scene/waitFor { since }` immediately or park it — reading the live
    /// [`SceneRevision`] under the same lock [`wake`](Self::wake) takes, so no
    /// bump can slip between the decision and the park (the lost-wakeup the
    /// naive check-then-park has).
    ///
    /// * `revision.current() > since` — the scene already advanced past the
    ///   client's baseline: answer at once with [`waiter_response`] (a no-`id`
    ///   notification gets no response, matching the parked path). The reply is
    ///   sent **after** releasing the lock.
    /// * otherwise — park the reply until a future bump wakes it.
    ///
    /// # Panics
    ///
    /// Panics only if the internal lock is poisoned (a prior holder panicked
    /// while mutating the list) — an unrecoverable invariant break, not a
    /// runtime condition a caller can provoke.
    pub fn park_if_current(
        &self,
        revision: &SceneRevision,
        since: u64,
        id: Option<RequestId>,
        reply: RpcReply,
    ) {
        let mut parked = self.parked.lock().expect("waiter registry lock poisoned");
        let current = revision.current();
        if current > since {
            // Already advanced. Release the lock before firing the reply (a
            // reply sink is opaque — never run it under the registry mutex).
            drop(parked);
            if let Some(id) = id {
                reply.send(waiter_response(Some(id), current));
            }
        } else {
            parked.push(Parked { since, id, reply });
        }
    }

    /// Wake every parked waiter the `revision` has surpassed (`since <
    /// revision`), firing each reply with the [`waiter_response`] carrying that
    /// revision. Returns the number woken.
    ///
    /// The [`SceneRevision`] wake observer calls this with the just-bumped
    /// value, so `revision` is already published before this acquires the
    /// parked lock — the "bump before lock" half of the no-lost-wakeup
    /// discipline. Satisfied replies are drained under the lock and fired
    /// **outside** it, so a reply sink that re-enters the registry cannot
    /// deadlock.
    ///
    /// # Panics
    ///
    /// Panics only if the internal lock is poisoned (see
    /// [`park_if_current`](Self::park_if_current)).
    pub fn wake(&self, revision: u64) -> usize {
        let woken: Vec<Parked> = {
            let mut parked = self.parked.lock().expect("waiter registry lock poisoned");
            let mut kept = Vec::with_capacity(parked.len());
            let mut fire = Vec::new();
            for w in parked.drain(..) {
                if w.since < revision {
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
            // A notification (no id) gets no response per JSON-RPC.
            if let Some(id) = w.id {
                w.reply.send(waiter_response(Some(id), revision));
            }
        }
        count
    }

    /// The number of currently parked waiters — for embedder introspection
    /// and tests.
    ///
    /// # Panics
    ///
    /// Panics only if the internal lock is poisoned (see
    /// [`park_if_current`](Self::park_if_current)).
    #[must_use]
    pub fn parked_count(&self) -> usize {
        self.parked
            .lock()
            .expect("waiter registry lock poisoned")
            .len()
    }
}

/// The shared **async `scene/waitFor` decision** the dispatch ingress routes a
/// frame through before normal dispatch (R1270).
///
/// A `scene/waitFor { since: <revision> }` is the async form (§6.3): the client
/// passes the [`SceneRevision`] it last observed and blocks until the scene
/// advances past it. This function claims exactly that shape:
///
/// * **Not an async waitFor** (a different method, or a `scene/waitFor` with no
///   numeric `since` — the v0 `{path, target, max_attempts}` busy-poll) →
///   [`ControlFlow::Continue(reply)`], handing the reply back so the caller runs
///   normal [`dispatch`](crate::dispatch::dispatch) (the v0 path stays
///   byte-unchanged when no `since` is supplied).
/// * **An async waitFor** → [`park_if_current`](WaiterRegistry::park_if_current)
///   answers-or-parks it and this returns [`ControlFlow::Break`].
///
/// `ControlFlow` rather than `Result` because "not my frame, here is your
/// reply back" is control flow, not failure — the un-consumed [`RpcReply`] is a
/// resource handed back, not an error.
///
/// The caller invokes this only when it has a registry + the scene's
/// [`SceneRevision`]; a `since`-shaped request on a backend with no registry
/// falls through to the v0 handler (which reports the missing `{path, target}`
/// params). Read the current revision non-blockingly with the `scene/revision`
/// method before issuing a blocking wait.
pub fn try_async_wait_for(
    request: &Request,
    revision: &SceneRevision,
    registry: &WaiterRegistry,
    reply: RpcReply,
) -> ControlFlow<(), RpcReply> {
    if request.method != "scene/waitFor" {
        return ControlFlow::Continue(reply);
    }
    let Some(since) = request
        .params
        .as_ref()
        .and_then(|p| p.get("since"))
        .and_then(serde_json::Value::as_u64)
    else {
        // No numeric `since` — the v0 synchronous busy-poll shape.
        return ControlFlow::Continue(reply);
    };
    registry.park_if_current(revision, since, request.id.clone(), reply);
    ControlFlow::Break(())
}

/// Build the JSON-RPC 2.0 success response a satisfied `scene/waitFor` returns,
/// carrying the [`SceneRevision`] the scene has advanced to. The single SSOT
/// for both the immediate-satisfaction path (baseline already stale at
/// dispatch) and the parked-then-woken path, serialized through the dispatch
/// `serialize` SSOT so a woken wait's wire frame is built exactly like every
/// synchronous response: `{ "changed": true, "revision": <n> }`.
#[must_use]
pub fn waiter_response(id: Option<RequestId>, revision: u64) -> String {
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Barrier, Mutex as StdMutex};

    use serde_json::json;

    /// A recording reply sink: pushes whatever string it is sent into a shared
    /// vec, so a test can assert what (if anything) a parked reply fired.
    fn recording_reply(sink: &Arc<StdMutex<Vec<String>>>) -> RpcReply {
        let sink = Arc::clone(sink);
        RpcReply::new(move |s| sink.lock().unwrap().push(s))
    }

    /// A counting reply sink for the concurrency stress test.
    fn counting_reply(fired: &Arc<AtomicU64>) -> RpcReply {
        let fired = Arc::clone(fired);
        RpcReply::new(move |_| {
            fired.fetch_add(1, Ordering::Relaxed);
        })
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
    fn park_at_current_then_bump_wakes_it() {
        let rev = SceneRevision::new(); // current = 0
        let registry = WaiterRegistry::new();
        let s = Arc::new(StdMutex::new(Vec::new()));
        // since = 0, current = 0 → not advanced → park.
        registry.park_if_current(&rev, 0, Some(RequestId::Num(1)), recording_reply(&s));
        assert_eq!(registry.parked_count(), 1);
        assert!(s.lock().unwrap().is_empty(), "parked, not answered yet");
        // A scene bump wakes it with the new revision.
        assert_eq!(registry.wake(rev.bump()), 1);
        assert_eq!(registry.parked_count(), 0);
        let v: serde_json::Value = serde_json::from_str(&s.lock().unwrap()[0]).unwrap();
        assert_eq!(v["result"]["revision"], 1);
    }

    #[test]
    fn park_when_baseline_stale_answers_immediately() {
        let rev = SceneRevision::new();
        rev.bump();
        rev.bump(); // current = 2
        let registry = WaiterRegistry::new();
        let s = Arc::new(StdMutex::new(Vec::new()));
        // since = 0, current = 2 → already advanced → immediate, no park.
        registry.park_if_current(&rev, 0, Some(RequestId::Num(1)), recording_reply(&s));
        assert_eq!(registry.parked_count(), 0, "a stale baseline does not park");
        let v: serde_json::Value = serde_json::from_str(&s.lock().unwrap()[0]).unwrap();
        assert_eq!(v["result"]["revision"], 2, "carries the current revision");
    }

    #[test]
    fn wake_fires_only_the_waiters_the_revision_surpassed() {
        let rev = SceneRevision::new();
        let registry = WaiterRegistry::new();
        let a = Arc::new(StdMutex::new(Vec::new())); // since 0
        let b = Arc::new(StdMutex::new(Vec::new())); // since 1
        registry.park_if_current(&rev, 0, Some(RequestId::Num(1)), recording_reply(&a));
        registry.park_if_current(&rev, 1, Some(RequestId::Num(2)), recording_reply(&b));
        assert_eq!(registry.parked_count(), 2);
        // Revision 1 surpasses since=0 (a), not since=1 (b).
        assert_eq!(registry.wake(1), 1);
        assert_eq!(a.lock().unwrap().len(), 1);
        assert!(
            b.lock().unwrap().is_empty(),
            "since=1 still parked at revision 1"
        );
        assert_eq!(registry.wake(2), 1, "revision 2 now surpasses since=1");
        assert_eq!(b.lock().unwrap().len(), 1);
        assert_eq!(registry.parked_count(), 0);
    }

    #[test]
    fn notification_waiter_is_dropped_without_a_response_on_both_paths() {
        let rev = SceneRevision::new();
        let registry = WaiterRegistry::new();
        // Parked path: no-id waiter woken → dropped, no send.
        let s = Arc::new(StdMutex::new(Vec::new()));
        registry.park_if_current(&rev, 0, None, recording_reply(&s));
        assert_eq!(registry.wake(rev.bump()), 1);
        assert!(
            s.lock().unwrap().is_empty(),
            "parked notification: no response"
        );
        // Immediate path: stale-baseline no-id waiter → dropped, no send (C2:
        // symmetric with the parked path, was a spurious id:null before R1270).
        let s2 = Arc::new(StdMutex::new(Vec::new()));
        registry.park_if_current(&rev, 0, None, recording_reply(&s2));
        assert_eq!(registry.parked_count(), 0, "stale baseline did not park");
        assert!(
            s2.lock().unwrap().is_empty(),
            "immediate notification: no response"
        );
    }

    #[test]
    fn decision_hands_back_non_wait_for_and_since_less_wait_for() {
        let rev = SceneRevision::new();
        let registry = WaiterRegistry::new();
        let other = Request {
            jsonrpc: "2.0".to_string(),
            method: "scene/query".to_string(),
            params: None,
            id: Some(RequestId::Num(1)),
        };
        let s = Arc::new(StdMutex::new(Vec::new()));
        assert!(
            matches!(
                try_async_wait_for(&other, &rev, &registry, recording_reply(&s)),
                ControlFlow::Continue(_)
            ),
            "a non-waitFor method is handed back"
        );
        let s2 = Arc::new(StdMutex::new(Vec::new()));
        assert!(
            matches!(
                try_async_wait_for(
                    &wait_for_request(None),
                    &rev,
                    &registry,
                    recording_reply(&s2)
                ),
                ControlFlow::Continue(_)
            ),
            "a since-less waitFor falls through to the v0 handler"
        );
        assert_eq!(
            registry.parked_count(),
            0,
            "nothing parked on the hand-back paths"
        );
    }

    #[test]
    fn decision_break_parks_and_a_bump_wakes_it() {
        let rev = SceneRevision::new();
        let registry = WaiterRegistry::new();
        let s = Arc::new(StdMutex::new(Vec::new()));
        assert!(
            matches!(
                try_async_wait_for(
                    &wait_for_request(Some(0)),
                    &rev,
                    &registry,
                    recording_reply(&s)
                ),
                ControlFlow::Break(())
            ),
            "an async waitFor is claimed"
        );
        assert_eq!(registry.parked_count(), 1);
        registry.wake(rev.bump());
        let v: serde_json::Value = serde_json::from_str(&s.lock().unwrap()[0]).unwrap();
        assert_eq!(v["result"]["revision"], 1);
    }

    #[test]
    fn concurrent_park_and_wake_never_loses_a_wakeup() {
        // The R1270 lost-wakeup regression guard. A bumper thread bumps the
        // shared revision (whose observer wakes the registry) at maximum
        // contention with the main thread parking at the pre-bump revision.
        // With the check-and-park-under-one-lock discipline, EVERY waiter must
        // fire (immediately if the bump won the race, woken if the park won) —
        // so `fired == iterations` deterministically. The pre-R1270 read-then-
        // park race would drop some wakeups here.
        let rev = Arc::new(SceneRevision::new());
        let registry = Arc::new(WaiterRegistry::new());
        {
            let r = Arc::clone(&registry);
            rev.set_observer(move |n| {
                r.wake(n);
            });
        }
        let fired = Arc::new(AtomicU64::new(0));
        let iterations = 3000u64;
        for i in 0..iterations {
            let since = rev.current();
            let barrier = Arc::new(Barrier::new(2));
            let bumper = {
                let r = Arc::clone(&rev);
                let b = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    b.wait();
                    r.bump();
                })
            };
            barrier.wait();
            registry.park_if_current(
                &rev,
                since,
                Some(RequestId::Num(i64::try_from(i).unwrap_or(i64::MAX))),
                counting_reply(&fired),
            );
            bumper.join().unwrap();
        }
        assert_eq!(
            fired.load(Ordering::Relaxed),
            iterations,
            "every waiter fired — no lost wakeup under concurrency",
        );
        assert_eq!(registry.parked_count(), 0, "no waiter left parked");
    }
}
