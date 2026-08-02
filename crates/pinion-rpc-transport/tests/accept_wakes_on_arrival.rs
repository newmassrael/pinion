//! R1541 §5.7 — the accept loop waits on the *listener*, not on a clock.
//!
//! The field report this round answers (sprag PR-81, measured 2026-08-02):
//! a `sprag` CLI invocation opens a fresh connection per call, and **99.5% of
//! its wall time was one constant** — the accept loop's 50 ms poll interval.
//! Same server, same request, same box: 0.025 ms per request on a warm
//! connection versus 50.188 ms on a cold one, a 2,000x gap whose p10/p90 band
//! (50.021 / 50.332) is the signature of a *timer* rather than of work.
//!
//! Two kinds of assertion live here, and the split is deliberate:
//!
//! - **Counter assertions** ([`TransportControl::accept_wakeups`]) state the
//!   property machine-independently — an idle endpoint wakes `0` times, an
//!   arrival wakes it exactly once, and an idle lifetime plus a shutdown
//!   wakes it exactly once. No clock reads into these, so they cannot flake
//!   on a loaded host, and a timer-polled loop fails every one of them.
//! - **One latency assertion**, because latency is what the field report
//!   actually measured and a counter cannot state it. It is bounded on the
//!   *median* of many cold round-trips, and the bound has two-sided margin:
//!   the fixed path measures a 36 us median (a ~690x margin below the bound)
//!   while the defect's whole distribution sits at 50.0-50.4 ms (a 2x margin
//!   above it). See the test for why that is not a host-reading threshold.
//!
//! Both were measured on the same box before and after the fix, by this file:
//! median 50,141 us (min 49,955 / max 50,366) polled, 36 us (min 32 / max
//! 250) waiting on readiness.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use pinion_rpc::{RpcFrame, RpcIngress};
use pinion_rpc_transport::{Exposure, UnixSocketTransport};

/// Long enough that a 50 ms poll interval would tick several times inside it,
/// short enough to keep the suite fast. Brackets an arrival to prove the
/// wakeup count does **not** move while nothing arrives — see
/// `one_arrival_is_one_wakeup` for why that bracket is load-bearing rather
/// than decorative.
const IDLE: Duration = Duration::from_millis(150);

/// Samples per arm of the latency measurement.
const SAMPLES: usize = 21;

/// Inline echo ingress: answers each frame on the submitting thread, so a
/// round-trip measures the transport and nothing else.
struct EchoIngress;

impl RpcIngress for EchoIngress {
    fn submit(&self, frame: RpcFrame) {
        let RpcFrame { request, reply, .. } = frame;
        reply.send(format!("echo:{request}"));
    }
}

fn unique_socket_path() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pinion-rpc-accept-{}-{n}.sock", std::process::id()))
}

/// Connect with a bounded retry: `serve` returns before the accept thread has
/// necessarily reached its first wait, so the very first connect can race.
fn connect(path: &Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(path) {
            Ok(s) => return s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(1)),
            Err(e) => panic!("connect to {} failed: {e}", path.display()),
        }
    }
}

/// One cold round-trip: a *fresh* connection carrying one request, which is
/// exactly the shape of a `sprag` CLI invocation.
fn cold_round_trip(path: &Path) -> String {
    let stream = connect(path);
    let mut w = &stream;
    w.write_all(b"ping\n").unwrap();
    w.flush().unwrap();
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).unwrap();
    line.trim_end().to_owned()
}

#[test]
fn an_idle_endpoint_never_wakes() {
    // Acceptance criterion 3 of the field report: idle wakeups go from ~20/s
    // to none. Stated as a counter rather than as a duration, so the
    // assertion is `0` versus `not 0` — the strongest discrimination
    // available and one no host speed can blur.
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    std::thread::sleep(Duration::from_millis(250));

    assert_eq!(
        control.accept_wakeups(),
        0,
        "an endpoint nobody connected to did work anyway",
    );
    drop(control);
}

#[test]
fn an_idle_endpoint_still_idles_while_withdrawn() {
    // The exposure axis does not smuggle a timer back in: a withdrawn
    // endpoint is bound and refusing service, and it does that by waiting,
    // not by waking up 20 times a second to re-read a flag it could have
    // read when something arrived.
    let path = unique_socket_path();
    let control =
        UnixSocketTransport::serve_with_exposure(&path, Arc::new(EchoIngress), Exposure::Withdrawn)
            .unwrap();

    std::thread::sleep(Duration::from_millis(250));

    assert_eq!(
        control.accept_wakeups(),
        0,
        "a withdrawn endpoint idles too"
    );
    drop(control);
}

#[test]
fn one_arrival_is_one_wakeup() {
    // The other half of "demand-driven": the loop wakes for arrivals, and it
    // wakes *once* per arrival.
    //
    // The idle brackets are not padding — without them this test PASSES on a
    // 50 ms timer-polled loop, which is how it was first written and what a
    // counterfactual caught. The reason is a coincidence worth naming: on the
    // timer implementation the client spends its whole round-trip waiting out
    // exactly one interval, so the count reads `1` there too. Bracketing the
    // arrival with idle periods states the property the bare count cannot —
    // the counter moves for arrivals and for nothing else.
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    std::thread::sleep(IDLE);
    assert_eq!(
        control.accept_wakeups(),
        0,
        "idling before the arrival woke it"
    );

    assert_eq!(cold_round_trip(&path), "echo:ping");
    assert_eq!(
        control.accept_wakeups(),
        1,
        "exactly one wake, and it was the arrival's",
    );

    std::thread::sleep(IDLE);
    assert_eq!(
        control.accept_wakeups(),
        1,
        "idling after the arrival woke it again",
    );
    drop(control);
}

#[test]
fn three_arrivals_are_three_wakeups() {
    // The count tracks arrivals rather than being pinned at a constant: a
    // counter that always read `1` would satisfy the test above and say
    // nothing. Each connection is fully round-tripped before the next opens,
    // so the three arrive as three separate readiness events rather than
    // racing into one drained batch — and each is checked as it lands, so a
    // count that drifted between arrivals is caught at the arrival after it
    // rather than being absorbed into the total.
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    std::thread::sleep(IDLE);
    assert_eq!(
        control.accept_wakeups(),
        0,
        "idling before the first arrival"
    );

    for expected in 1..=3 {
        assert_eq!(cold_round_trip(&path), "echo:ping");
        assert_eq!(
            control.accept_wakeups(),
            expected,
            "wake {expected} should be arrival {expected}'s, and only it",
        );
    }
    drop(control);
}

#[test]
fn an_endpoint_that_only_ever_shuts_down_wakes_exactly_once() {
    // Acceptance criterion 2, deterministically: shutdown ends the accept
    // loop *now* rather than at the end of a poll interval. A duration
    // assertion here would be weak — the defect's shutdown latency is
    // uniform on 0..50 ms, so any bound either flakes or fails to
    // discriminate. The count does discriminate absolutely: over this
    // endpoint's whole life the loop woke for one reason, and it was the
    // shutdown knocking.
    let path = unique_socket_path();
    let mut control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    std::thread::sleep(Duration::from_millis(250));
    control.shutdown();

    assert_eq!(
        control.accept_wakeups(),
        1,
        "the only wake in this endpoint's life should be its own shutdown",
    );
}

#[test]
fn a_cold_connection_is_served_without_waiting_for_a_clock() {
    // Acceptance criterion 1, and the only assertion here that reads a clock.
    //
    // Why it is not a host-reading threshold: the failure it guards against
    // is a *fixed timer*, so the failing distribution is not slow work that a
    // fast machine could outrun — it is a constant at the poll interval, and
    // the field report's p10/p90 of 50.021/50.332 ms is how narrow that band
    // is. The bound therefore has margin on BOTH sides: ~690x above the
    // fixed path's measured median and ~2x below the defect's fastest
    // sample. The median (not the max) is compared so one descheduled
    // sample cannot decide the verdict.
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    // Warm the path once so process-first-touch costs (thread spawn, page
    // faults) are not attributed to the measurement.
    assert_eq!(cold_round_trip(&path), "echo:ping");

    let mut micros: Vec<u128> = (0..SAMPLES)
        .map(|_| {
            let start = Instant::now();
            assert_eq!(cold_round_trip(&path), "echo:ping");
            start.elapsed().as_micros()
        })
        .collect();
    micros.sort_unstable();
    let median = micros[SAMPLES / 2];

    assert!(
        median < 25_000,
        "a fresh connection waited on a clock: median {median} us over {SAMPLES} cold \
         round-trips (min {} us, max {} us); the accept loop is expected to answer \
         readiness, not a poll interval",
        micros[0],
        micros[SAMPLES - 1],
    );
    drop(control);
}
