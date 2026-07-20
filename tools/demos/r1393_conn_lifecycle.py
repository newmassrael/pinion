#!/usr/bin/env python3
"""R1393 §5.7 — PINION-PR67: RPC connection identity + lifecycle, over the wire.

The forcing consumer for the seam sprag needs to track *live* per-connection
state crash-safely (tmux `detach-on-destroy no-detached` / a `list-clients`
"N viewers" badge). Before R1393 a `RpcIngress` saw only `submit(frame)` and an
`RpcFrame` carried no connection id, so a stateful ingress could neither
attribute a frame to its connection nor learn when a connection closed. R1393
adds exactly two edges:

  * R-67.1 — every `RpcFrame` carries a `ConnId` (opaque, process-unique).
  * R-67.2 — `RpcIngress` gained `on_connect` / `on_disconnect` (default no-op);
             the Unix-socket transport fires `on_disconnect` when a connection's
             reader ends however the client dies (EOF / reset / crash).

`hello-conn-lifecycle` mounts the socket transport behind a stateful
`AttachTrackingIngress` that maintains a live-connection set and reflects it into
the scene as data: a `role=status` line "N connections attached" and a `list` of
the opaque live ids. This demo drives the whole PINION-PR67 round-trip over TWO
transports at once:

  * the built-in stdin channel (the harness) is the out-of-band OBSERVER — it
    reads `scene/snapshot` and is deliberately NOT counted (mirroring sprag,
    where the control channel is not a tracked attachment);
  * raw `AF_UNIX` socket connections are the tracked CLIENTS — opening one
    raises the count and adds its id; a frame sent over it dispatches through the
    real core and its response routes back to THAT socket (R-67.1); closing it
    — by a plain `close()`, the crash analog — drops the count and removes
    exactly that id (R-67.2, attribution).

  (A) baseline — the observer sees 0 attached; the placeholder row is present.
  (B) open A — count rises to 1, one id appears; a frame over A round-trips.
  (C) open B — 2 distinct ids (A's still present; monotonic A < B).
  (D) close A — count drops to 1 and the SURVIVING id is B's (the right
      connection was cleaned up), never A's.
  (E) close B — count returns to 0 and the placeholder is back.

Every observation reads the scene as data (§2 #7) over the §5.12 plane — no
pixels. ZERO-FLAKE: bounded `wait_snap` polling (never a fixed sleep) on the
observed post-action state; a private per-pid socket path. >=30 assertions.
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

STATUS_TAG = "conn_status"
LIST_TAG = "conn_list"
HINT_TAG = "conn_hint"


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


def status_text(snap: Any) -> Optional[str]:
    node = find_by_tag(snap, STATUS_TAG)
    if node is None:
        return None
    texts = _texts_of(node)
    return texts[0] if texts else None


def status_count(snap: Any) -> Optional[int]:
    text = status_text(snap)
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


def list_texts(snap: Any) -> list[str]:
    node = find_by_tag(snap, LIST_TAG)
    return _texts_of(node) if node is not None else []


# ── a raw AF_UNIX client connection (the tracked-client analog) ─────────────


class SockClient:
    """One line-framed JSON-RPC 2.0 connection straight to the app's socket —
    a tracked client, distinct from the harness's stdin observer channel."""

    def __init__(self, path: str) -> None:
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(5.0)
        # `serve` returns before the accept thread has necessarily reached its
        # first `accept()`, so a connect can race the bind — bounded retry.
        deadline = time.monotonic() + 5.0
        while True:
            try:
                self.sock.connect(path)
                break
            except OSError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.02)
        self._buf = b""

    def rpc(self, method: str, params: Any = None, rid: int = 1) -> dict:
        env: dict[str, Any] = {"jsonrpc": "2.0", "id": rid, "method": method}
        if params is not None:
            env["params"] = params
        self.sock.sendall((json.dumps(env) + "\n").encode())
        while b"\n" not in self._buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise AssertionError("socket closed before a response arrived")
            self._buf += chunk
        line, self._buf = self._buf.split(b"\n", 1)
        return json.loads(line.decode())

    def close(self) -> None:
        # A plain close is the crash analog: the server's reader hits EOF and
        # must fire on_disconnect for THIS connection's id.
        self.sock.close()


def body() -> None:
    sock_path = Path(tempfile.gettempdir()) / f"pinion-r1393-conn-{os.getpid()}.sock"
    # Start from a clean path (a crashed prior run could leave a stale file;
    # the app also removes it before binding).
    try:
        sock_path.unlink()
    except FileNotFoundError:
        pass

    clients: list[SockClient] = []
    try:
        with RpcSubprocess(EXAMPLE, env={SOCK_ENV: str(sock_path)}, boot_grace=1.0) as tf:
            # ── (A) baseline: the observer sees 0 attached ──────────────────
            snap = wait_snap(
                tf,
                lambda s: status_count(s) == 0,
                source="paint",
                desc="baseline 0 connections",
            )
            assert_eq(status_count(snap), 0, "baseline count")
            assert_eq(status_text(snap), "0 connections attached", "baseline status text")
            assert_eq(live_ids(snap), [], "baseline: no live ids")
            assert find_by_tag(snap, STATUS_TAG) is not None, "status region present"
            assert find_by_tag(snap, LIST_TAG) is not None, "connections list present"
            assert any(
                "No connections" in t for t in list_texts(snap)
            ), "empty list shows the placeholder row"
            # The stdin observer channel is itself a connection to the app, yet
            # the count is 0 — it is deliberately not a tracked (socket) client.
            assert_eq(status_count(snap), 0, "stdin observer channel is not counted")
            # The hint line echoes the socket path this app is bound to.
            hint = find_by_tag(snap, HINT_TAG)
            assert hint is not None, "hint region present"
            assert any(
                str(sock_path) in t for t in _texts_of(hint)
            ), "hint shows the bound socket path"
            assert sock_path.exists(), "the app bound the Unix socket file"

            # ── (B) open client A + a frame over it round-trips ─────────────
            a = SockClient(str(sock_path))
            clients.append(a)
            resp_a = a.rpc("scene/snapshot", {"path": "", "from": "state"}, rid=11)
            assert_eq(resp_a.get("id"), 11, "A: response echoes the request id")
            assert "result" in resp_a, "A: a frame over the socket dispatched + routed back"
            assert "error" not in resp_a, "A: the socket frame succeeded"

            snap = wait_snap(
                tf,
                lambda s: status_count(s) == 1,
                source="paint",
                desc="count rises to 1 after A opens",
            )
            assert_eq(status_count(snap), 1, "count after A")
            assert_eq(status_text(snap), "1 connection attached", "A: singular status text")
            ids_1 = live_ids(snap)
            assert_eq(len(ids_1), 1, "exactly one live id after A")
            id_a = ids_1[0]
            assert id_a > 0, "a live ConnId is a positive opaque value"
            assert any(
                t == f"conn #{id_a}" for t in list_texts(snap)
            ), "A's id row renders via the conn_row_text SSOT"

            # ── (C) open client B: two distinct ids ─────────────────────────
            b = SockClient(str(sock_path))
            clients.append(b)
            resp_b = b.rpc("scene/snapshot", {"path": "", "from": "state"}, rid=22)
            assert_eq(resp_b.get("id"), 22, "B: response echoes the request id")
            assert "result" in resp_b, "B: a frame over the socket dispatched + routed back"

            snap = wait_snap(
                tf,
                lambda s: status_count(s) == 2,
                source="paint",
                desc="count rises to 2 after B opens",
            )
            assert_eq(status_count(snap), 2, "count after B")
            assert_eq(status_text(snap), "2 connections attached", "B: plural status text")
            ids_2 = live_ids(snap)
            assert_eq(len(ids_2), 2, "two live ids after B")
            assert id_a in ids_2, "A's id persists while A stays open"
            others = [x for x in ids_2 if x != id_a]
            assert_eq(len(others), 1, "the second id is distinct from A's")
            id_b = others[0]
            assert id_a != id_b, "concurrent connections get distinct ids (R-67.1)"
            assert id_a < id_b, "ids are monotonic — A opened before B"

            # ── (D) close A: count drops, the SURVIVING id is B's ───────────
            a.close()
            clients.remove(a)
            snap = wait_snap(
                tf,
                lambda s: status_count(s) == 1,
                source="paint",
                desc="count drops to 1 after A closes",
            )
            assert_eq(status_count(snap), 1, "count after A closes")
            assert_eq(
                status_text(snap), "1 connection attached", "close A: back to singular"
            )
            ids_3 = live_ids(snap)
            assert_eq(ids_3, [id_b], "A cleaned up crash-safely; exactly B survives (R-67.2)")
            assert id_a not in ids_3, "A's id is gone — the right connection was released"

            # ── (E) close B: back to zero ───────────────────────────────────
            b.close()
            clients.remove(b)
            snap = wait_snap(
                tf,
                lambda s: status_count(s) == 0,
                source="paint",
                desc="count returns to 0 after B closes",
            )
            assert_eq(status_count(snap), 0, "count after all clients close")
            assert_eq(status_text(snap), "0 connections attached", "final status text")
            assert_eq(live_ids(snap), [], "no live ids remain")
            assert any(
                "No connections" in t for t in list_texts(snap)
            ), "the placeholder row is back once empty"
    finally:
        for c in clients:
            c.close()
        try:
            sock_path.unlink()
        except FileNotFoundError:
            pass


if __name__ == "__main__":
    sys.exit(run_demo("R1393 §5.7 PINION-PR67 RPC connection lifecycle", body))
