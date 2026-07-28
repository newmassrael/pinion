#!/usr/bin/env python3
"""R1469 §5.7 — PINION-PR48: the RPC endpoint's exposure is declared at BIND.

The consumer requirement (sprag `APP_RPC=off`): bind the socket but refuse
service — "always there, withdrawn". Before PR-48 the only sequence
`pinion-rpc-transport` allowed was `serve()`, which binds *serving* and arms
the accept loop, followed by the consumer withdrawing once it got the control
back. Everything the consumer does in between is a window, and the cost of
landing in it is not one instant of exposure: the connection is accepted while
serving, and withdrawing deliberately does not evict in-flight connections, so
it is served for its WHOLE SESSION — by an endpoint the operator asked to be
withdrawn.

PR-48 makes the exposure an argument to the bind
(`UnixSocketTransport::serve_with_exposure`), so the flag the accept loop reads
is created from the declared policy before the loop exists.

`hello-conn-lifecycle` is the reference consumer: `PINION_CONN_LIFECYCLE_EXPOSURE`
declares the boot exposure, `main` hands it to the bind, and the live exposure is
reflected into the scene as data (§2 #7) on the `conn_exposure` region — read
off the `TransportControl` itself, so it answers "is this endpoint serving?" and
not "what did it boot as?".

This demo boots the SAME binary twice and contrasts the two policies:

  (A) withdrawn — the scene says "withdrawn (bound, refusing service)"; the
      socket FILE exists (it really is bound); a real AF_UNIX client connects
      at the OS level and is closed without a response; the attached count
      never leaves 0 and no id ever appears; and the out-of-band stdin channel
      keeps answering throughout — which is how an agent reads the state of an
      app whose socket is withdrawn at all.
  (B) serving — same binary, same socket path shape, exposure declared
      `serving`: the scene says so, and that same client attaches (count 1,
      one live id) and round-trips a frame over the socket, then detaches.

The A/B is what makes (A) a measurement rather than an absence: every refusal
in (A) is a step that demonstrably succeeds in (B).

Honest limit, stated rather than papered over: the sub-microsecond window
INSIDE the old sequence has no external witness — a run cannot deterministically
land a client in it, and a ZERO-FLAKE demo will not race for one. What is
measured here is the consumer-visible contract (the declared policy holds from
the first client onward); the session-outlives-the-withdraw half is pinned
deterministically by `pinion-rpc-transport`'s
`a_post_bind_withdraw_leaves_a_session_it_meant_to_refuse`.

Every observation reads the scene as data over the §5.12 plane — no pixels.
ZERO-FLAKE: bounded `wait_snap` polling (never a fixed sleep), a private
per-pid socket path per phase. >=30 assertions.

Run from the workspace root:
    cargo build -p hello-conn-lifecycle --release
    python3 tools/demos/r1469_transport_exposure.py
"""

from __future__ import annotations

import json
import os
import re
import socket
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

EXAMPLE = "hello-conn-lifecycle"
SOCK_ENV = "PINION_CONN_LIFECYCLE_SOCK"
EXPOSURE_ENV = "PINION_CONN_LIFECYCLE_EXPOSURE"

STATUS_TAG = "conn_status"
LIST_TAG = "conn_list"
EXPOSURE_TAG = "conn_exposure"

SERVING_LINE = "Endpoint: serving"
WITHDRAWN_LINE = "Endpoint: withdrawn (bound, refusing service)"

# How long to give a withdrawn endpoint to close a connection it refuses. The
# accept loop polls the non-blocking listener every 50 ms (ACCEPT_POLL), so a
# refusal costs at most one poll plus scheduling; this is orders of magnitude
# over that, and it is an upper BOUND, not a sleep — a refusal returns as soon
# as it happens.
REFUSAL_TIMEOUT = 5.0


# ── scene-as-data extraction (matches the binding's SSOT text fns) ──────────


def _texts_of(node: Any) -> list[str]:
    """All `Text.content` strings under `node`, depth-first."""
    out: list[str] = []
    if not isinstance(node, dict):
        return out
    content = node.get("content")
    if isinstance(content, str):
        out.append(content)
    children = node.get("children")
    if isinstance(children, list):
        for child in children:
            out.extend(_texts_of(child))
    if isinstance(content, dict):  # Scroll.content is a node, not a string
        out.extend(_texts_of(content))
    return out


def text_of(snap: Any, tag: str) -> Optional[str]:
    node = find_by_tag(snap, tag)
    if node is None:
        return None
    texts = _texts_of(node)
    return texts[0] if texts else None


def exposure_access_name(tf: RpcSubprocess) -> Optional[str]:
    """`scene/access` -> the accessible name of the exposure live region.

    The a11y surface is checked as well as the paint because the exposure is a
    `role=status` live region: an assistive technology should ANNOUNCE a
    withdrawal, and the name must come from the same SSOT the paint uses (a
    drifted duplicate would still render correctly and lie to AT)."""
    for node in tf.request("scene/access").result["nodes"]:
        if node.get("tag") == EXPOSURE_TAG:
            return node.get("name")
    return None


def status_count(snap: Any) -> Optional[int]:
    text = text_of(snap, STATUS_TAG)
    if text is None:
        return None
    m = re.match(r"(\d+) connection", text)
    return int(m.group(1)) if m else None


def live_ids(snap: Any) -> list[int]:
    """The sorted opaque ConnId values the list rows render."""
    node = find_by_tag(snap, LIST_TAG)
    if node is None:
        return []
    ids: list[int] = []
    for text in _texts_of(node):
        m = re.match(r"conn #(\d+)", text)
        if m:
            ids.append(int(m.group(1)))
    return sorted(ids)


# ── a raw AF_UNIX client, the thing an exposure policy admits or refuses ────


def connect_socket(path: str) -> socket.socket:
    """Open a real AF_UNIX connection to `path`, retrying only while the file
    is still absent (the app binds during boot). Succeeding here says nothing
    about exposure: a withdrawn endpoint is BOUND, so the OS-level connect
    completes either way — the server's response is what differs."""
    deadline = time.monotonic() + 5.0
    while True:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(REFUSAL_TIMEOUT)
        try:
            sock.connect(path)
            return sock
        except OSError:
            sock.close()
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.02)


def rpc_over_socket(sock: socket.socket, method: str, params: Any, rid: int) -> Optional[dict]:
    """Send one JSON-RPC frame over `sock` and return the response, or `None`
    if the endpoint refused service (closed the connection / reset it)."""
    env: dict[str, Any] = {"jsonrpc": "2.0", "id": rid, "method": method}
    if params is not None:
        env["params"] = params
    try:
        sock.sendall((json.dumps(env) + "\n").encode())
    except OSError:
        # The refusal beat our write. The read below is the real check, but
        # there is nothing left to read on a socket that is already gone.
        return None
    buf = b""
    while b"\n" not in buf:
        try:
            chunk = sock.recv(4096)
        except (ConnectionResetError, TimeoutError):
            return None
        if not chunk:  # EOF: closed with no response — refused.
            return None
        buf += chunk
    return json.loads(buf.split(b"\n", 1)[0].decode())


def fresh_socket_path(label: str) -> Path:
    path = Path(tempfile.gettempdir()) / f"pinion-r1469-{label}-{os.getpid()}.sock"
    try:
        path.unlink()
    except FileNotFoundError:
        pass
    return path


# ── (A) booted withdrawn ────────────────────────────────────────────────────


def phase_withdrawn(sock_path: Path) -> None:
    env = {SOCK_ENV: str(sock_path), EXPOSURE_ENV: "withdrawn"}
    with RpcSubprocess(EXAMPLE, env=env, boot_grace=1.0) as tf:
        snap = wait_snap(
            tf,
            lambda s: text_of(s, EXPOSURE_TAG) is not None,
            source="paint",
            desc="the withdrawn app paints its exposure",
        )

        # The endpoint reports its policy as data, over the out-of-band
        # channel — the only channel a withdrawn endpoint leaves open.
        assert find_by_tag(snap, EXPOSURE_TAG) is not None, "exposure region present"
        assert_eq(text_of(snap, EXPOSURE_TAG), WITHDRAWN_LINE, "boot exposure is withdrawn")
        assert WITHDRAWN_LINE != SERVING_LINE, "the two policies render differently"

        # Withdrawn is NOT "not listening": the socket really is bound.
        assert sock_path.exists(), "a withdrawn endpoint still BOUND its socket file"
        assert sock_path.is_socket(), "the bound path is a socket, not a stray file"

        # The app is fully alive on the observer channel.
        assert_eq(status_count(snap), 0, "withdrawn: nothing attached")
        assert_eq(live_ids(snap), [], "withdrawn: no live ids")
        assert find_by_tag(snap, STATUS_TAG) is not None, "status region present"
        assert find_by_tag(snap, LIST_TAG) is not None, "connections list present"

        # A real client: the OS-level connect succeeds (bound) ...
        sock = connect_socket(str(sock_path))
        try:
            # ... and the endpoint refuses to serve it.
            resp = rpc_over_socket(sock, "scene/snapshot", {"path": "", "from": "state"}, 11)
            assert resp is None, "withdrawn: the client's frame got NO response"
        finally:
            sock.close()

        # A refused client was never admitted: no id, no count, no trace. The
        # count is re-read AFTER the refusal, so this is an observed absence.
        snap = tf.snapshot(source="paint")
        assert_eq(status_count(snap), 0, "withdrawn: the refused client never attached")
        assert_eq(live_ids(snap), [], "withdrawn: no id was ever issued to it")
        assert_eq(text_of(snap, EXPOSURE_TAG), WITHDRAWN_LINE, "still withdrawn after the refusal")

        # A second client fares exactly the same — the refusal is the policy,
        # not a one-off boot artefact.
        sock2 = connect_socket(str(sock_path))
        try:
            resp2 = rpc_over_socket(sock2, "scene/snapshot", {"path": "", "from": "state"}, 12)
            assert resp2 is None, "withdrawn: a second client is refused too"
        finally:
            sock2.close()
        snap = tf.snapshot(source="paint")
        assert_eq(status_count(snap), 0, "withdrawn: still nothing attached")

        # The observer channel answered every one of those reads, so the app
        # was never merely dead: refusing service is a live, deliberate state.
        assert_eq(text_of(snap, EXPOSURE_TAG), WITHDRAWN_LINE, "withdrawn is a LIVE state")

        # AT reads the same policy, from the same SSOT as the paint.
        assert_eq(
            exposure_access_name(tf),
            WITHDRAWN_LINE,
            "withdrawn: the role=status live region announces the refusal",
        )
        assert_eq(
            exposure_access_name(tf),
            text_of(snap, EXPOSURE_TAG),
            "withdrawn: the accessible name and the paint share one SSOT",
        )


# ── (B) booted serving — the same binary, the same client, admitted ─────────


def phase_serving(sock_path: Path) -> None:
    env = {SOCK_ENV: str(sock_path), EXPOSURE_ENV: "serving"}
    with RpcSubprocess(EXAMPLE, env=env, boot_grace=1.0) as tf:
        snap = wait_snap(
            tf,
            lambda s: text_of(s, EXPOSURE_TAG) is not None,
            source="paint",
            desc="the serving app paints its exposure",
        )
        assert_eq(text_of(snap, EXPOSURE_TAG), SERVING_LINE, "boot exposure is serving")
        assert sock_path.exists(), "the serving endpoint bound its socket file"
        assert_eq(status_count(snap), 0, "serving: nothing attached yet")
        assert_eq(live_ids(snap), [], "serving: no live ids yet")

        sock = connect_socket(str(sock_path))
        try:
            # The step that returned None in (A) returns a real response here.
            resp = rpc_over_socket(sock, "scene/snapshot", {"path": "", "from": "state"}, 21)
            assert resp is not None, "serving: the identical client IS served"
            assert_eq(resp.get("id"), 21, "serving: the response echoes the request id")
            assert "result" in resp, "serving: the frame dispatched through the real core"
            assert "error" not in resp, "serving: the socket frame succeeded"

            snap = wait_snap(
                tf,
                lambda s: status_count(s) == 1,
                source="paint",
                desc="serving: the client attaches",
            )
            assert_eq(status_count(snap), 1, "serving: count rises to 1")
            ids = live_ids(snap)
            assert_eq(len(ids), 1, "serving: exactly one live id")
            assert ids[0] > 0, "serving: a live ConnId is a positive opaque value"
            assert_eq(text_of(snap, EXPOSURE_TAG), SERVING_LINE, "still serving while attached")
        finally:
            sock.close()

        # Closing detaches it crash-safely (R-PR67), and the exposure — a
        # property of the endpoint, not of any client — is unchanged.
        snap = wait_snap(
            tf,
            lambda s: status_count(s) == 0,
            source="paint",
            desc="serving: the client detaches",
        )
        assert_eq(status_count(snap), 0, "serving: count returns to 0")
        assert_eq(live_ids(snap), [], "serving: no live ids remain")
        assert_eq(text_of(snap, EXPOSURE_TAG), SERVING_LINE, "exposure survives a detach")
        assert_eq(
            exposure_access_name(tf),
            SERVING_LINE,
            "serving: the live region announces the serving policy",
        )
        assert exposure_access_name(tf) != WITHDRAWN_LINE, (
            "the two policies are distinguishable to AT, not just in the paint"
        )


def body() -> None:
    withdrawn_path = fresh_socket_path("withdrawn")
    serving_path = fresh_socket_path("serving")
    try:
        phase_withdrawn(withdrawn_path)
        phase_serving(serving_path)
    finally:
        for path in (withdrawn_path, serving_path):
            try:
                path.unlink()
            except FileNotFoundError:
                pass


if __name__ == "__main__":
    sys.exit(run_demo("R1469 §5.7 PINION-PR48 RPC endpoint boot exposure", body))
