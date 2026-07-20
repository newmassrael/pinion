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
use pinion_rpc_transport::UnixSocketTransport;

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
