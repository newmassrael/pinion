#!/usr/bin/env python3
"""R1478 §5.7 — a fixed-path RPC endpoint is an IDENTITY, not a slot.

R1469 (PINION-PR48) landed the endpoint's exposure at bind and found, in
passing, a neighbouring defect it deliberately left out of scope: the transport
unlinked whatever sat at the socket path before binding its own. On a *stale*
path that is the point — a fixed-path endpoint has to survive a crashed run. On
a *live* path it is a silent takeover, and silent is the operative word:

  * every later client reaching that path is served by the newcomer, while the
    incumbent keeps a listener nobody can ever reach again;
  * neither process reports anything. The incumbent's bind is still `Ok`, its
    accept loop still polls, and its clients simply stop arriving.

The same missing invariant had a second half at teardown: a departing endpoint
removed *the path*, not *the socket file it had bound*, so it could delete a
successor's endpoint on its way out.

R1478 states it once — an endpoint owns a NAME, and only ever binds or unbinds
its own — and `hello-endpoint-identity` is the reference consumer: it does not
`expect` its bind, it reports the outcome into the scene as data (§2 #7) on the
`endpoint_state` `role=status` region, next to its own `endpoint_label`.

The label is what makes ownership MEASURABLE rather than argued. Two instances
aimed at one path render different labels, so a raw `AF_UNIX` client that
connects to the socket and reads the snapshot back learns *which process* is
behind the name — the exact question a takeover answers wrongly.

Five phases:

  (A) alpha binds a private path — the scene says bound, the socket file
      exists, and a real client is served BY ALPHA (its label comes back).
  (B) beta boots at the SAME path while alpha lives — the bind is refused, the
      refusal is readable as data over beta's out-of-band channel (the only
      channel an app without an endpoint has left), and that same client is
      still served by ALPHA, not beta.
  (C) beta exits — a bind it never won takes nothing with it: alpha's socket
      file is still there and alpha still answers.
  (D) alpha exits, gamma boots at that path — it binds, and clients now reach
      GAMMA. The refusal in (B) was ownership, not poisoning.
  (E) a deterministically stale path (a socket file bound and closed by this
      script, with nobody behind it) is still reclaimed — the behaviour the old
      unconditional unlink existed for, preserved.

The A/B contrast is what makes (B) a measurement rather than an absence: every
refusal in (B) is a step that demonstrably succeeds in (A) and (D).

A discriminator this demo deliberately does NOT use, having measured it: the
socket file's inode. "The name did not change hands" looks like it should be an
inode comparison, but these sockets live on a `tmpfs`, and a tmpfs hands the
freed inode number straight back to the next `bind` — four bind/unlink cycles
on one path here returned the same `st_ino` every time. An inode check would
therefore pass just as happily for a file that HAD been replaced, which is no
check at all. Asking the endpoint who it is (`label_over_socket`) measures the
thing the inode was only ever a proxy for.

Honest note: the liveness probe costs the incumbent one accepted-and-closed
connection (an `on_connect`/`on_disconnect` pair carrying no frames). That is
the price of the only liveness test `AF_UNIX` offers without a side-channel
lock file, and it is paid only by a bind that was about to displace something.
This example does not track connections, so the probe is invisible here; the
lifecycle hooks it would fire are `hello-conn-lifecycle`'s subject.

Every observation reads the scene as data over the §5.12 plane — no pixels.
ZERO-FLAKE: bounded `wait_snap` polling (never a fixed sleep), private per-pid
socket paths, and a staged hand-over rather than a raced one. >=30 assertions.

Run from the workspace root:
    cargo build -p hello-endpoint-identity --release
    python3 tools/demos/r1478_endpoint_identity.py
"""

from __future__ import annotations

import os
import socket
import sys
import tempfile
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    SocketClient,
    assert_eq,
    find_by_tag,
    run_demo,
    texts_of,
    wait_snap,
)

EXAMPLE = "hello-endpoint-identity"
SOCK_ENV = "PINION_ENDPOINT_IDENTITY_SOCK"
LABEL_ENV = "PINION_ENDPOINT_IDENTITY_LABEL"

STATE_TAG = "endpoint_state"
LABEL_TAG = "endpoint_label"


def bound_line(path: Path) -> str:
    return f"Endpoint: bound at {path}"


def contested_line(path: Path) -> str:
    return f"Endpoint: refused — {path} is held by a live endpoint"


# ── scene-as-data extraction (matches the binding's SSOT text fns) ──────────


def text_of(snap: Any, tag: str) -> Optional[str]:
    node = find_by_tag(snap, tag)
    if node is None:
        return None
    texts = texts_of(node)
    return texts[0] if texts else None


def access_name(tf: RpcSubprocess, tag: str) -> Optional[str]:
    """`scene/access` -> the accessible name of a live region.

    Checked alongside the paint because the claim is a `role=status` region: an
    assistive technology should ANNOUNCE that an app could not take its
    endpoint, and the name must come from the same SSOT the paint uses (a
    drifted duplicate would render correctly and lie to AT)."""
    for node in tf.request("scene/access").result["nodes"]:
        if node.get("tag") == tag:
            return node.get("name")
    return None


def label_over_socket(sock_path: Path, rid: int) -> Optional[str]:
    """Ask the app BEHIND the socket path who it is.

    This is the ownership measurement: the answer comes from whichever process
    the kernel routes this path to, so a takeover would answer with the wrong
    label rather than merely failing."""
    with SocketClient(sock_path) as client:
        resp = client.rpc("scene/snapshot", {"path": "", "from": "paint"}, rid)
        if resp is None:
            return None
        assert "result" in resp, f"the socket frame dispatched (rid={rid})"
        return text_of(resp["result"], LABEL_TAG)


def boot(label: str, sock_path: Path) -> RpcSubprocess:
    return RpcSubprocess(
        EXAMPLE,
        env={SOCK_ENV: str(sock_path), LABEL_ENV: label},
        boot_grace=1.0,
    )


def claim_of(tf: RpcSubprocess, desc: str) -> tuple[Any, str]:
    """Wait until the instance has painted a resolved endpoint claim, and
    return `(snapshot, claim_line)`. The wait is on the claim being resolved —
    never on a fixed sleep — because the bind happens on the ingress hook and
    the first paint can precede it."""
    snap = wait_snap(
        tf,
        lambda s: (text_of(s, STATE_TAG) or "").find("not yet claimed") == -1
        and text_of(s, STATE_TAG) is not None,
        source="paint",
        desc=desc,
    )
    claim = text_of(snap, STATE_TAG)
    assert claim is not None, f"{desc}: the claim region is present"
    return snap, claim


def fresh_socket_path(label: str) -> Path:
    path = Path(tempfile.gettempdir()) / f"pinion-r1478-{label}-{os.getpid()}.sock"
    try:
        path.unlink()
    except FileNotFoundError:
        pass
    return path


def body() -> None:
    contested_path = fresh_socket_path("contested")
    stale_path = fresh_socket_path("stale")
    try:
        phases_a_to_d(contested_path)
        phase_stale(stale_path)
    finally:
        for path in (contested_path, stale_path):
            try:
                path.unlink()
            except FileNotFoundError:
                pass


def phases_a_to_d(sock_path: Path) -> None:
    # ── (A) alpha takes the name ────────────────────────────────────────────
    with boot("alpha", sock_path) as alpha:
        snap, claim = claim_of(alpha, "alpha claims its endpoint")
        assert find_by_tag(snap, STATE_TAG) is not None, "alpha: claim region present"
        assert_eq(claim, bound_line(sock_path), "alpha: bound at its path")
        assert_eq(text_of(snap, LABEL_TAG), "Instance: alpha", "alpha: names itself")
        assert sock_path.exists(), "alpha: the socket file exists"
        assert sock_path.is_socket(), "alpha: the bound path is a socket, not a stray file"
        assert_eq(
            access_name(alpha, STATE_TAG),
            claim,
            "alpha: AT and the paint read one SSOT",
        )

        # A real client is served, and says WHO served it.
        assert_eq(
            label_over_socket(sock_path, 11),
            "Instance: alpha",
            "alpha: a client at this path reaches alpha",
        )

        # ── (B) beta is refused, and says so ────────────────────────────────
        with boot("beta", sock_path) as beta:
            snap_b, claim_b = claim_of(beta, "beta resolves its endpoint claim")
            assert_eq(claim_b, contested_line(sock_path), "beta: the bind was REFUSED")
            assert claim_b != bound_line(sock_path), "beta: refusal is not a bind"
            assert_eq(text_of(snap_b, LABEL_TAG), "Instance: beta", "beta: names itself")
            assert str(sock_path) in claim_b, "beta: the refusal names the contested path"

            # The whole §2 #7 point: the app that could NOT take the endpoint
            # is exactly the app you cannot ask over that endpoint — so it
            # answers on the out-of-band channel instead, with structured data.
            assert_eq(
                access_name(beta, STATE_TAG),
                claim_b,
                "beta: AT is told about the refusal too",
            )
            snap_b2 = beta.snapshot(source="paint")
            assert_eq(
                text_of(snap_b2, STATE_TAG),
                claim_b,
                "beta: the refusal is a LIVE readable state, not a boot log line",
            )

            # The name never changed hands.
            assert sock_path.exists(), "beta: the socket file survived the refused bind"

            # ... and the incumbent is undisturbed.
            snap_a = alpha.snapshot(source="paint")
            assert_eq(
                text_of(snap_a, STATE_TAG),
                bound_line(sock_path),
                "alpha: still bound while contested",
            )

            # The discriminating measurement: a client at that path still
            # reaches ALPHA. Under the pre-R1478 unlink it would reach beta.
            served_by = label_over_socket(sock_path, 12)
            assert_eq(served_by, "Instance: alpha", "beta: clients still reach the INCUMBENT")
            assert served_by != "Instance: beta", "beta: the newcomer serves nobody"

        # ── (C) beta departs, taking nothing that was not its own ───────────
        assert sock_path.exists(), "beta's exit left alpha's socket file alone"
        assert_eq(
            label_over_socket(sock_path, 13),
            "Instance: alpha",
            "alpha still owns its endpoint after beta exits",
        )
        snap_a = alpha.snapshot(source="paint")
        assert_eq(
            text_of(snap_a, STATE_TAG), bound_line(sock_path), "alpha: claim unchanged"
        )

    # ── (D) the incumbent leaves; the path is claimable again ───────────────
    with boot("gamma", sock_path) as gamma:
        snap_g, claim_g = claim_of(gamma, "gamma claims the vacated endpoint")
        assert_eq(claim_g, bound_line(sock_path), "gamma: the path was not poisoned")
        assert_eq(text_of(snap_g, LABEL_TAG), "Instance: gamma", "gamma: names itself")
        assert_eq(
            label_over_socket(sock_path, 21),
            "Instance: gamma",
            "gamma: clients now reach gamma",
        )
        assert_eq(
            access_name(gamma, STATE_TAG), claim_g, "gamma: AT and the paint agree"
        )


def phase_stale(sock_path: Path) -> None:
    """(E) The behaviour the old unconditional unlink existed for, kept.

    The leftover is manufactured rather than crashed into existence: binding
    and closing an `AF_UNIX` socket leaves its file behind with nobody
    listening, which is exactly what a killed process leaves. Deterministic —
    no crash is simulated and nothing is timed."""
    leftover = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    leftover.bind(str(sock_path))
    leftover.close()
    assert sock_path.exists(), "the stale precondition: a socket file is present"
    assert sock_path.is_socket(), "the stale precondition: it really is a socket"
    with boot("delta", sock_path) as delta:
        _, claim = claim_of(delta, "delta reclaims a stale path")
        assert_eq(claim, bound_line(sock_path), "delta: a stale path is still bindable")
        assert_eq(
            label_over_socket(sock_path, 31),
            "Instance: delta",
            "delta: the reclaimed endpoint serves",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1478 §5.7 RPC endpoint identity at bind", body))
