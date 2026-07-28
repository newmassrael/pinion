//! §5.7 PR-47 — end-to-end proof that the Unix-socket transport reads a
//! frame, drives it through the [`RpcIngress`] seam, and routes the
//! response back to the *originating* connection — winit-free, no shell.
//!
//! The mock ingress dispatches inline (echoing the request) so the test
//! exercises the transport plumbing — accept, per-connection framing, and
//! per-connection reply routing — without a live UI thread. ZERO-FLAKE:
//! a private socket under the temp dir, deterministic request/response.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pinion_rpc::{ConnId, RpcFrame, RpcIngress};
use pinion_rpc_transport::{Exposure, UnixSocketTransport};

/// Inline echo ingress: answers each frame with `echo:<request>` on the
/// submitting thread. Stands in for the shell's UI-thread dispatch.
struct EchoIngress;

impl RpcIngress for EchoIngress {
    fn submit(&self, frame: RpcFrame) {
        let RpcFrame { request, reply, .. } = frame;
        reply.send(format!("echo:{request}"));
    }
}

/// Ingress that never replies (models a JSON-RPC notification: no `id`,
/// no response). Used to prove a no-response frame leaves the connection
/// intact rather than hanging the writer.
struct SilentIngress;

impl RpcIngress for SilentIngress {
    fn submit(&self, frame: RpcFrame) {
        // Drop the reply unused — no response is written.
        drop(frame.reply);
    }
}

/// R-PR67 — one recorded lifecycle event, capturing the `ConnId` (as its
/// raw value) the transport attributed it to.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Life {
    Connect(u64),
    Frame(u64, String),
    Disconnect(u64),
}

/// R-PR67 lifecycle-recording ingress: logs each `on_connect` / `submit` /
/// `on_disconnect` in arrival order with the connection id, so a test can
/// assert the transport fires the hooks with the right ids in the right
/// order. Echoes each frame (like `EchoIngress`) so the client still gets a
/// response — proving frames flow while the lifecycle is tracked.
#[derive(Default)]
struct RecordingIngress {
    events: Mutex<Vec<Life>>,
}

impl RecordingIngress {
    fn events(&self) -> Vec<Life> {
        self.events.lock().unwrap().clone()
    }
}

impl RpcIngress for RecordingIngress {
    fn submit(&self, frame: RpcFrame) {
        let RpcFrame {
            conn,
            request,
            reply,
        } = frame;
        // Record the frame BEFORE producing the echo, so a client that has
        // read the response is guaranteed the frame is already logged.
        self.events
            .lock()
            .unwrap()
            .push(Life::Frame(conn.get(), request.clone()));
        reply.send(format!("echo:{request}"));
    }

    fn on_connect(&self, conn: ConnId) {
        self.events.lock().unwrap().push(Life::Connect(conn.get()));
    }

    fn on_disconnect(&self, conn: ConnId) {
        self.events
            .lock()
            .unwrap()
            .push(Life::Disconnect(conn.get()));
    }
}

/// Poll the recorder (bounded, deterministic — not a sleep-and-hope) until
/// `done` holds over the recorded events, then return them.
fn poll_until(recorder: &Arc<RecordingIngress>, done: impl Fn(&[Life]) -> bool) -> Vec<Life> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let events = recorder.events();
        if done(&events) {
            return events;
        }
        assert!(
            Instant::now() < deadline,
            "lifecycle condition never met; recorded: {events:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The conn id the transport attributed to the frame carrying `request`.
fn frame_conn(recorder: &Arc<RecordingIngress>, request: &str) -> Option<u64> {
    recorder.events().iter().find_map(|e| match e {
        Life::Frame(c, r) if r == request => Some(*c),
        _ => None,
    })
}

fn unique_socket_path() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pinion-rpc-transport-{}-{n}.sock",
        std::process::id()
    ))
}

/// Connect with a short retry: `serve` returns before the accept thread
/// has necessarily reached its first `accept()`, so the first connect can
/// race. Bounded, deterministic — not a sleep-and-hope.
fn connect(path: &std::path::Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(path) {
            Ok(s) => return s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => panic!("connect to {} failed: {e}", path.display()),
        }
    }
}

fn send_line(stream: &mut UnixStream, line: &str) {
    // Best-effort: writing to a connection the server is refusing (the
    // disabled-endpoint case) can race the peer's reset and fail with
    // EPIPE/ECONNRESET. The subsequent `read_line` assertion is the real
    // check — a positive-case write that silently failed surfaces there as
    // a missing response, not a hidden pass.
    let _ = stream.write_all(line.as_bytes());
    let _ = stream.write_all(b"\n");
    let _ = stream.flush();
}

fn read_line(stream: &UnixStream) -> Option<String> {
    use std::io::ErrorKind::{BrokenPipe, ConnectionReset, UnexpectedEof};
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    match reader.read_line(&mut buf) {
        Ok(0) => None, // EOF: connection closed with no response.
        Ok(_) => Some(buf.trim_end().to_owned()),
        // A server that refuses service closes the connection; depending
        // on whether our unread request bytes were still queued, the OS
        // surfaces that as a clean EOF (above) or a reset — both mean "no
        // response served", which is exactly what these tests assert.
        Err(e) if matches!(e.kind(), ConnectionReset | BrokenPipe | UnexpectedEof) => None,
        Err(e) => panic!("read failed: {e}"),
    }
}

#[test]
fn frame_response_routes_back_to_the_connection() {
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    let mut stream = connect(&path);
    send_line(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":1,"method":"scene/snapshot"}"#,
    );
    let response = read_line(&stream).expect("a response line");
    assert_eq!(
        response,
        r#"echo:{"jsonrpc":"2.0","id":1,"method":"scene/snapshot"}"#
    );

    drop(control);
}

#[test]
fn two_connections_each_receive_their_own_response() {
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    let mut a = connect(&path);
    let mut b = connect(&path);
    send_line(&mut a, "req-A");
    send_line(&mut b, "req-B");

    // Each connection's response is its own request echoed back — no
    // cross-talk between the two concurrent connections.
    assert_eq!(read_line(&a).unwrap(), "echo:req-A");
    assert_eq!(read_line(&b).unwrap(), "echo:req-B");

    drop(control);
}

#[test]
fn notification_frame_writes_no_response() {
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(SilentIngress)).unwrap();

    let mut stream = connect(&path);
    send_line(&mut stream, "a-notification");
    // Close our write side so the server's reader hits EOF and the writer
    // thread's channel drains + closes; our read then sees EOF with no
    // bytes — a notification produced no response, and nothing hung.
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(read_line(&stream), None);

    drop(control);
}

#[test]
fn disabled_endpoint_refuses_new_connections() {
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    control.disable();
    assert!(!control.is_enabled());

    // While disabled the socket stays bound (connect may succeed at the
    // OS level) but the server closes the connection without serving, so
    // a request gets no response — the endpoint refuses service.
    let mut stream = connect(&path);
    send_line(&mut stream, "req-while-disabled");
    assert_eq!(read_line(&stream), None);

    // Re-enable and the same endpoint serves again — runtime on/off.
    control.enable();
    let mut stream = connect(&path);
    send_line(&mut stream, "req-after-enable");
    assert_eq!(read_line(&stream).unwrap(), "echo:req-after-enable");

    drop(control);
}

// ─── R-PR48: the endpoint's exposure is declared at bind ────────────────────

#[test]
fn a_withdrawn_bind_admits_no_session_at_all() {
    // R-PR48 — the endpoint sprag's `APP_RPC=off` policy asks for: the socket
    // is bound (the path exists, `connect` succeeds at the OS level) and the
    // very first client is refused. The recorder is the discriminating half —
    // `on_connect` only fires from `handle_connection`, which the accept loop
    // spawns only while serving, so an empty log means no session was ever
    // admitted, not merely that no reply came back.
    let path = unique_socket_path();
    let recorder = Arc::new(RecordingIngress::default());
    let control =
        UnixSocketTransport::serve_with_exposure(&path, recorder.clone(), Exposure::Withdrawn)
            .unwrap();

    assert!(path.exists(), "a withdrawn endpoint is still BOUND");

    let mut stream = connect(&path);
    send_line(&mut stream, "would-have-landed-in-the-window");
    // The `None` here is the accept loop having closed the connection, so by
    // this point the accept has definitely happened — the emptiness below is
    // an observed refusal, not an unobserved race.
    assert_eq!(read_line(&stream), None, "the first client is refused");
    assert_eq!(
        recorder.events(),
        Vec::new(),
        "no on_connect, no frame: the ingress never saw a session",
    );

    drop(control);
}

#[test]
fn a_post_bind_withdraw_leaves_a_session_it_meant_to_refuse() {
    // R-PR48 — WHY the exposure has to be part of the bind, reproduced
    // deterministically: this is the only sequence the pre-PR48 API allowed a
    // withdrawn-at-boot consumer — bind serving, then withdraw once the
    // control comes back. Whatever the consumer does in between is window;
    // here we simply connect IN it rather than racing it.
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    let mut early = connect(&path);
    send_line(&mut early, "landed-in-the-window");
    assert_eq!(read_line(&early).unwrap(), "echo:landed-in-the-window");

    // The consumer now applies the boot policy it wanted all along.
    control.set_exposure(Exposure::Withdrawn);
    assert_eq!(control.exposure(), Exposure::Withdrawn);

    // New connections are refused from here on ...
    let mut late = connect(&path);
    send_line(&mut late, "after-the-withdraw");
    assert_eq!(read_line(&late), None, "later clients are refused");

    // ... but the session admitted in the window keeps being served, because
    // withdrawing refuses future admissions and deliberately does not evict
    // live ones. So the cost of the window is not "microseconds of exposure":
    // it is one unintended session, open for as long as its client holds it.
    send_line(&mut early, "still-served-after-the-withdraw");
    assert_eq!(
        read_line(&early).unwrap(),
        "echo:still-served-after-the-withdraw",
        "the in-window session outlives the withdraw",
    );

    drop(control);
}

#[test]
fn a_withdrawn_bind_is_a_starting_point_not_a_lock() {
    // The boot exposure sets where the endpoint starts; the runtime toggle is
    // unchanged, so an app booted withdrawn can still be exposed later
    // (sprag's `APP_RPC=off` then an operator opting in).
    let path = unique_socket_path();
    let control =
        UnixSocketTransport::serve_with_exposure(&path, Arc::new(EchoIngress), Exposure::Withdrawn)
            .unwrap();

    let mut refused = connect(&path);
    send_line(&mut refused, "before-exposing");
    assert_eq!(read_line(&refused), None);

    control.set_exposure(Exposure::Serving);
    let mut served = connect(&path);
    send_line(&mut served, "after-exposing");
    assert_eq!(read_line(&served).unwrap(), "echo:after-exposing");

    drop(control);
}

#[test]
fn the_bare_serve_binds_serving() {
    // The pre-PR48 contract is unchanged: `serve` is the `Exposure::Serving`
    // shorthand, and says so in the type a consumer reads back.
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();
    assert_eq!(control.exposure(), Exposure::Serving);

    let mut stream = connect(&path);
    send_line(&mut stream, "served-with-no-exposure-argument");
    assert_eq!(
        read_line(&stream).unwrap(),
        "echo:served-with-no-exposure-argument"
    );

    drop(control);
}

#[test]
fn the_bool_view_and_the_typed_view_are_one_state() {
    // Read/write symmetry across the two vocabularies: whichever one writes,
    // both read the same state back.
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();
    assert_eq!(control.exposure(), Exposure::Serving);
    assert!(control.is_enabled());

    control.set_enabled(false);
    assert_eq!(
        control.exposure(),
        Exposure::Withdrawn,
        "a bool write reads back typed",
    );

    control.set_exposure(Exposure::Serving);
    assert!(control.is_enabled(), "a typed write reads back as the bool");

    control.disable();
    assert_eq!(control.exposure(), Exposure::Withdrawn);
    control.enable();
    assert_eq!(control.exposure(), Exposure::Serving);

    drop(control);
}

#[test]
fn drop_unbinds_the_socket_path() {
    let path = unique_socket_path();
    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();
    assert_eq!(control.path(), path.as_path());
    // Prove it was serving.
    let _ = connect(&path);
    drop(control);

    // After drop the accept thread removes the socket file and stops
    // serving; a fresh connect no longer reaches a server. A connect that
    // Errs (socket gone) exits the loop — the expected steady state.
    let deadline = Instant::now() + Duration::from_secs(5);
    while let Ok(mut s) = UnixStream::connect(&path) {
        // Rare race: the socket file lingered a beat. A served connection
        // would echo; a torn-down one yields EOF.
        let _ = s.write_all(b"ping\n");
        let _ = s.flush();
        if read_line(&s).is_none() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "endpoint still serving after drop"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn lifecycle_connect_frames_then_disconnect() {
    // R-PR67 — a connection's whole life, observed by a stateful ingress:
    // one on_connect, then every frame stamped with that id, then exactly
    // one on_disconnect when the client goes away — in order.
    let path = unique_socket_path();
    let recorder = Arc::new(RecordingIngress::default());
    let control = UnixSocketTransport::serve(&path, recorder.clone()).unwrap();

    let mut stream = connect(&path);
    send_line(&mut stream, "req-1");
    assert_eq!(read_line(&stream).unwrap(), "echo:req-1");
    send_line(&mut stream, "req-2");
    assert_eq!(read_line(&stream).unwrap(), "echo:req-2");

    // Close the client fully: the server's reader hits EOF and must fire
    // on_disconnect within a bounded time — the crash-safe cleanup edge.
    stream.shutdown(std::net::Shutdown::Both).unwrap();
    let events = poll_until(&recorder, |e| {
        e.iter().any(|x| matches!(x, Life::Disconnect(_)))
    });

    // The id is opaque, but the same one threads the whole sequence.
    let conn = match events.first() {
        Some(Life::Connect(c)) => *c,
        other => panic!("first lifecycle event must be Connect, got {other:?}"),
    };
    assert_eq!(
        events,
        vec![
            Life::Connect(conn),
            Life::Frame(conn, "req-1".to_owned()),
            Life::Frame(conn, "req-2".to_owned()),
            Life::Disconnect(conn),
        ],
        "connect, then both frames on that id, then disconnect — in order",
    );

    drop(control);
}

#[test]
fn two_connections_get_distinct_ids_and_independent_disconnect() {
    // R-PR67 — concurrent connections are attributed distinct ids, and
    // closing one fires only that one's disconnect: the crash-safe cleanup
    // targets the connection that actually went away (the sprag
    // per-session-attachment refcount case).
    let path = unique_socket_path();
    let recorder = Arc::new(RecordingIngress::default());
    let control = UnixSocketTransport::serve(&path, recorder.clone()).unwrap();

    let mut a = connect(&path);
    send_line(&mut a, "from-A");
    assert_eq!(read_line(&a).unwrap(), "echo:from-A");
    let mut b = connect(&path);
    send_line(&mut b, "from-B");
    assert_eq!(read_line(&b).unwrap(), "echo:from-B");

    let id_a = frame_conn(&recorder, "from-A").expect("A's frame was attributed");
    let id_b = frame_conn(&recorder, "from-B").expect("B's frame was attributed");
    assert_ne!(id_a, id_b, "concurrent connections get distinct ids");

    // Close A only: A disconnects, B does not.
    a.shutdown(std::net::Shutdown::Both).unwrap();
    poll_until(&recorder, |e| e.contains(&Life::Disconnect(id_a)));
    assert!(
        !recorder.events().contains(&Life::Disconnect(id_b)),
        "B's disconnect must not fire while only A closed",
    );

    // Close B: now B disconnects with its own id.
    b.shutdown(std::net::Shutdown::Both).unwrap();
    poll_until(&recorder, |e| e.contains(&Life::Disconnect(id_b)));

    drop(control);
}

// ─── R1478: the endpoint's identity at the path ─────────────────────────────

#[test]
fn a_bind_refuses_to_displace_a_live_endpoint() {
    // R1478 — the path is the endpoint's NAME, and a name has one owner. A
    // second bind at a live path used to unlink the incumbent's socket file
    // and bind its own in place: every later client reached the newcomer,
    // while the incumbent kept a listener nobody could ever reach again.
    // Refusing is what makes a fixed-path endpoint an identity rather than a
    // last-writer-wins slot.
    let path = unique_socket_path();
    let incumbent = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    let Err(err) = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)) else {
        panic!("a live path must not be taken over");
    };
    assert_eq!(
        err.kind(),
        std::io::ErrorKind::AddrInUse,
        "the refusal is the OS's own vocabulary for a contested name",
    );

    // The incumbent still owns the name: only its listener is bound to this
    // path, so being served here proves the name never changed hands.
    let mut stream = connect(&path);
    send_line(&mut stream, "still-the-incumbent");
    assert_eq!(
        read_line(&stream).unwrap(),
        "echo:still-the-incumbent",
        "the incumbent kept serving its own path",
    );

    drop(incumbent);
}

#[test]
fn a_withdrawn_endpoint_still_owns_its_name() {
    // R1478 × R-PR48 — `Withdrawn` refuses SERVICE, not the name: the socket
    // is bound, which is the whole point of "always there, withdrawn". So a
    // second bind must be refused exactly as it is against a serving endpoint
    // — otherwise "bound but refusing service" would be a policy any other
    // process on the box could revoke by binding over it.
    //
    // This is also why the liveness probe is a `connect`: it succeeds against
    // a withdrawn endpoint (the listen backlog is what answers), so it tests
    // ownership of the name rather than willingness to serve.
    let path = unique_socket_path();
    let incumbent =
        UnixSocketTransport::serve_with_exposure(&path, Arc::new(EchoIngress), Exposure::Withdrawn)
            .unwrap();

    let Err(err) = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)) else {
        panic!("a withdrawn endpoint's name is still taken");
    };
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);

    // ... and it is still the withdrawn incumbent behind the name, not a
    // newcomer that quietly took it and serves.
    let mut stream = connect(&path);
    send_line(&mut stream, "would-be-served-by-a-usurper");
    assert_eq!(
        read_line(&stream),
        None,
        "the withdrawn incumbent still answers for this path",
    );

    drop(incumbent);
}

#[test]
fn a_bind_reclaims_a_stale_socket_file() {
    // R1478 negative control — the behaviour the old unconditional unlink was
    // FOR must survive: a crashed run leaves a socket file with nobody behind
    // it, and a fixed-path endpoint has to be re-bindable across that. Here
    // the leftover is made deterministically (std does not unlink an
    // `AF_UNIX` path when the listener drops), so no crash is simulated.
    let path = unique_socket_path();
    {
        let leftover = std::os::unix::net::UnixListener::bind(&path).unwrap();
        drop(leftover);
    }
    assert!(path.exists(), "the leftover file is the precondition");

    let control = UnixSocketTransport::serve(&path, Arc::new(EchoIngress))
        .expect("a stale path is still reclaimable");
    let mut stream = connect(&path);
    send_line(&mut stream, "after-reclaiming");
    assert_eq!(read_line(&stream).unwrap(), "echo:after-reclaiming");

    drop(control);
}

#[test]
fn a_departed_endpoint_never_unlinks_the_path_a_second_time() {
    // R1478 — the other half of the same invariant. Teardown used to unlink
    // the path TWICE (the accept thread on its way out, then `shutdown` after
    // the join), and between those two moments the path is free — so a
    // successor could bind it and the second unlink would delete the
    // successor's socket, leaving a live app nobody can reach.
    //
    // Staged rather than raced, and entirely through the public API: the
    // departing endpoint completes its release, a successor takes the name,
    // and only then does the departing control run its second teardown (the
    // `Drop` that always follows an explicit `shutdown`). ZERO-FLAKE — no
    // timing is relied on, and no external actor touches the path.
    let path = unique_socket_path();
    let mut departing = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    // The release: after this the name is free and the departing endpoint has
    // no further business with it.
    departing.shutdown();
    assert!(!path.exists(), "shutdown released the name");

    let successor = UnixSocketTransport::serve(&path, Arc::new(EchoIngress)).unwrap();

    // The second teardown — via `Drop`, exactly as a consumer's scope end
    // would run it.
    drop(departing);

    assert!(
        path.exists(),
        "the successor's socket file outlived the departed endpoint's Drop",
    );
    let mut stream = connect(&path);
    send_line(&mut stream, "successor-still-reachable");
    assert_eq!(
        read_line(&stream).unwrap(),
        "echo:successor-still-reachable",
        "a departed endpoint took only its own name with it",
    );

    drop(successor);
}
