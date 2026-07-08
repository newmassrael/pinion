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
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use pinion_rpc::{RpcFrame, RpcIngress};
use pinion_rpc_transport::UnixSocketTransport;

/// Inline echo ingress: answers each frame with `echo:<request>` on the
/// submitting thread. Stands in for the shell's UI-thread dispatch.
struct EchoIngress;

impl RpcIngress for EchoIngress {
    fn submit(&self, frame: RpcFrame) {
        let RpcFrame { request, reply } = frame;
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
