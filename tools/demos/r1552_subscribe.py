#!/usr/bin/env python3
"""R1552 §5.7 §6.3 §2 #2 §2 #7 — the server speaks first.

Before this round one frame could be answered at most once: `RpcReply` is a
`FnOnce` and `RpcFrame` held exactly one. So "one request, many answers" — a
change stream — was not merely unimplemented, it was **inexpressible** on this
transport at any price, and a client following the scene had to re-issue
`scene/waitFor` per revision, paying a round trip each time. That is what
PINION-PR83 reported.

R1552 adds `RpcEgress`, the connection's writer and the mirror of `RpcIngress`,
and `scene/subscribe` on top of it.

What this demo asserts, over the wire, against a real application:

  * a subscription answers ONCE and is then written to **unprompted** — the
    notification arrives with no request outstanding;
  * the frame is a JSON-RPC 2.0 *notification* (a `method`, no
    `id`), not a second Response. That is the whole design turning point: a
    client keyed on its own pending ids — which is every conforming client, and
    this project's own `rpc_verify` — resolves a request on the first matching
    `id` and discards the rest, so a repeated Response would be unreadable;
  * the revision the stream names is the revision the app **paints**, so the
    two introspection channels verify each other rather than restating one
    derivation;
  * a stream never names a subscription the client has not been told about —
    the arm-after-reply property, checked against every notification seen;
  * two connections get two independent streams, and closing one **without
    unsubscribing** — the crash case — releases exactly that one;
  * one connection cannot close another's stream;
  * `scene/subscriptions` publishes the live streams as data (§2 #7), which the toolkit
    has no equivalent of: nothing in the toolkit binds a server-initiated write to a
    named stream, so local server cannot be asked who is listening;
  * `rpc/methods` and `rpc/schema` cover this round's wire the day it lands.

ZERO-FLAKE: bounded `wait_until` / `await_notifications` polling (never a fixed
sleep). >=30 assertions.

Run from the workspace root:
    cargo build -p hello-subscribe --release
    python3 tools/demos/r1552_subscribe.py
"""

from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    SocketClient,
    assert_eq,
    assert_rpc_error,
    call,
    find_by_tag,
    run_demo,
    texts_of,
    wait_until,
)

EXAMPLE = "hello-subscribe"
SOCK_ENV = "PINION_SUBSCRIBE_SOCK"

CHANGED = "scene/changed"
REVISION_TAG = "sub_revision"
STREAMS_TAG = "sub_streams"
PUBLISHED_TAG = "sub_published"


def revision(tf: RpcSubprocess) -> int:
    return int(call(tf, "scene/revision")["revision"])


def painted_revision(tf: RpcSubprocess) -> int | None:
    """The revision the app is PAINTING, read out of the scene as data.

    Deliberately a second, independent channel from `scene/revision`: the
    stream's claim is only worth what an unrelated reading agrees with. Read
    from `from=paint` — the last RENDERED frame — so it lags a mutation by a
    repaint, which is why every caller polls it with `wait_until` rather than
    reading it once. `None` before the first paint.
    """
    node = find_by_tag(call(tf, "scene/snapshot", {"path": "", "from": "paint"}), REVISION_TAG)
    if node is None:
        return None
    for text in texts_of(node):
        if text.startswith("Scene revision: "):
            return int(text.removeprefix("Scene revision: "))
    return None


def status_text(tf: RpcSubprocess, tag: str) -> str | None:
    """One painted status line, or `None` before the first paint."""
    node = find_by_tag(call(tf, "scene/snapshot", {"path": "", "from": "paint"}), tag)
    if node is None:
        return None
    texts = texts_of(node)
    return texts[0] if texts else None


def body() -> None:
    sock_path = Path(tempfile.gettempdir()) / f"pinion-r1552-sub-{os.getpid()}.sock"
    try:
        sock_path.unlink()
    except FileNotFoundError:
        pass

    clients: list[SocketClient] = []
    try:
        with RpcSubprocess(EXAMPLE, env={SOCK_ENV: str(sock_path)}, boot_grace=1.0) as tf:
            # ── (A) baseline: nothing is subscribed, and that is stated ─────
            live = call(tf, "scene/subscriptions")
            assert_eq(live["subscriptions"], [], "A: no streams at boot")
            assert_eq(live["published_total"], 0, "A: nothing has been published")
            wait_until(
                lambda: status_text(tf, STREAMS_TAG) == "0 live change streams",
                desc="the boot paint reports no streams",
            )
            assert_eq(
                status_text(tf, PUBLISHED_TAG),
                "0 notifications published",
                "A: published total is scene data (§2 #7)",
            )
            assert_eq(tf.drain_notifications(CHANGED), [], "A: nothing spoke unprompted")

            # ── (B) one request opens the stream ────────────────────────────
            r0 = revision(tf)
            opened = call(tf, "scene/subscribe", {"since": r0})
            sub = opened["subscription"]
            assert sub >= 1, "B: subscription ids start at 1, so 0 is never live"
            assert_eq(opened["revision"], r0, "B: caught up to the `since` it was given")

            live = call(tf, "scene/subscriptions")
            assert_eq(len(live["subscriptions"]), 1, "B: one live stream")
            row = live["subscriptions"][0]
            assert_eq(row["subscription"], sub, "B: the id it answered with")
            assert_eq(row["armed"], True, "B: armed once its own response went out")
            assert_eq(row["delivered_count"], 0, "B: nothing delivered yet")
            assert_eq(row["revision"], r0, "B: its cursor is where it started")
            assert_eq(
                tf.drain_notifications(CHANGED),
                [],
                "B: subscribing is not itself a scene change",
            )

            # ── (C) the server writes without being asked ───────────────────
            tf.request("scene/tick", {"dt": 0.016})
            r1 = revision(tf)
            assert r1 > r0, f"C: the tick advanced the scene ({r0} -> {r1})"
            notes = tf.await_notifications(CHANGED, 1)
            assert_eq(len(notes), 1, "C: exactly one notification for one advance")

            # ── (D) and what it wrote is a NOTIFICATION, not a response ─────
            note = notes[0]
            assert_eq(note["jsonrpc"], "2.0", "D: a well-formed JSON-RPC frame")
            assert_eq(note["method"], CHANGED, "D: it carries a method")
            assert "id" not in note, "D: and NO id — JSON-RPC 2.0 section 4.1, not a response"
            assert "result" not in note, "D: not a response envelope"
            assert "error" not in note, "D: not an error envelope"
            assert_eq(note["params"]["subscription"], sub, "D: names its stream")
            assert_eq(note["params"]["revision"], r1, "D: and the revision reached")

            # ── (E) the two channels agree about what happened ──────────────
            wait_until(
                lambda: painted_revision(tf) == r1,
                desc="the painted revision catches the notified one",
            )
            assert_eq(painted_revision(tf), r1, "E: the app paints the revision it published")
            assert_eq(
                painted_revision(tf),
                note["params"]["revision"],
                "E: paint and stream name the SAME generation",
            )
            assert_eq(
                status_text(tf, STREAMS_TAG),
                "1 live change stream",
                "E: and the same paint reports the stream (singular)",
            )

            live = call(tf, "scene/subscriptions")
            assert_eq(live["subscriptions"][0]["delivered_count"], 1, "E: one delivered")
            assert_eq(live["published_total"], 1, "E: one published, process-wide")

            # ── (F) a stream never names an id the client was not told ──────
            for n in tf.notifications(CHANGED):
                assert_eq(
                    n["params"]["subscription"],
                    sub,
                    "F: every notification names a subscription this client opened",
                )

            # ── (G) closing it stops the stream, and reports what it did ────
            closed = call(tf, "scene/unsubscribe", {"subscription": sub})
            assert_eq(closed["subscription"], sub, "G: the id that was closed")
            assert_eq(closed["delivered_count"], 1, "G: what it delivered before closing")
            assert_eq(call(tf, "scene/subscriptions")["subscriptions"], [], "G: none live")

            before = len(tf.notifications(CHANGED))
            tf.request("scene/tick", {"dt": 0.016})
            assert revision(tf) > r1, "G: the scene really did advance again"
            assert_eq(
                len(tf.drain_notifications(CHANGED)),
                before,
                "G: a closed stream is silent",
            )
            assert_eq(
                call(tf, "scene/subscriptions")["published_total"],
                1,
                "G: the running total survives the stream that earned it",
            )

            # ── (H) closing twice is refused by name, not silently fine ─────
            assert_rpc_error(
                lambda: tf.request("scene/unsubscribe", {"subscription": sub}),
                code=-32602,
                data="unknown_subscription",
            )
            assert_rpc_error(
                lambda: tf.request("scene/subscribe", {"since": "soon"}),
                code=-32602,
                data="invalid_since",
            )
            assert_rpc_error(
                lambda: tf.request("scene/unsubscribe", {}),
                code=-32602,
                data="invalid_subscription_id",
            )

            # ── (I) two connections, two independent streams ────────────────
            assert sock_path.exists(), "I: the app bound its socket"
            a = SocketClient(sock_path)
            clients.append(a)
            b = SocketClient(sock_path)
            clients.append(b)

            r2 = revision(tf)
            sub_a = a.rpc("scene/subscribe", {"since": r2}, rid=11)["result"]["subscription"]
            sub_b = b.rpc("scene/subscribe", {"since": r2}, rid=21)["result"]["subscription"]
            assert sub_a != sub_b, "I: distinct streams get distinct ids"

            live = call(tf, "scene/subscriptions")
            assert_eq(len(live["subscriptions"]), 2, "I: two live streams")
            conns = {row["conn"] for row in live["subscriptions"]}
            assert_eq(len(conns), 2, "I: on two different connections")

            tf.request("scene/tick", {"dt": 0.016})
            r3 = revision(tf)
            note_a = a.await_notifications(CHANGED, 1)[0]
            note_b = b.await_notifications(CHANGED, 1)[0]
            assert_eq(note_a["params"]["subscription"], sub_a, "I: A hears about A")
            assert_eq(note_b["params"]["subscription"], sub_b, "I: B hears about B")
            assert_eq(note_a["params"]["revision"], r3, "I: both name the same advance")
            assert_eq(note_b["params"]["revision"], r3, "I: from one scene, one token")

            # ── (J) one connection cannot close another's stream ────────────
            refused = a.rpc("scene/unsubscribe", {"subscription": sub_b}, rid=12)
            assert_eq(
                refused["error"]["data"],
                "unknown_subscription",
                "J: B's stream is not A's to close",
            )
            assert_eq(
                len(call(tf, "scene/subscriptions")["subscriptions"]),
                2,
                "J: and it is still live",
            )

            # ── (K) a client that VANISHES has its stream released ──────────
            # The crash case: no `scene/unsubscribe` is ever sent.
            a.close()
            clients.remove(a)
            surviving = wait_until(
                lambda: call(tf, "scene/subscriptions")["subscriptions"],
                desc="the registry settles after the disconnect",
            )
            surviving = wait_until(
                lambda: (
                    call(tf, "scene/subscriptions")["subscriptions"]
                    if len(call(tf, "scene/subscriptions")["subscriptions"]) == 1
                    else None
                ),
                desc="exactly A's stream was released",
            )
            assert_eq(len(surviving), 1, "K: one stream left")
            assert_eq(surviving[0]["subscription"], sub_b, "K: and it is B's — attribution")

            # B still works, which is what "only that one" means.
            tf.request("scene/tick", {"dt": 0.016})
            r4 = revision(tf)
            note_b2 = b.await_notifications(CHANGED, 2)[-1]
            assert_eq(note_b2["params"]["revision"], r4, "K: B's stream is unharmed")

            # ── (L) the surface describes itself (§2 #7) ────────────────────
            methods = {m["name"]: m for m in call(tf, "rpc/methods")["methods"]}
            for name in ("scene/subscribe", "scene/unsubscribe", "scene/subscriptions"):
                assert name in methods, f"L: {name} is discoverable"
                assert_eq(methods[name]["occ"], "read", f"L: {name} mutates no scene")

            schema = {t["name"]: t for t in call(tf, "rpc/schema")["types"]}
            for name in (
                "SubscribeOutcome",
                "UnsubscribeOutcome",
                "SubscriptionView",
                "SubscriptionsOutcome",
            ):
                assert name in schema, f"L: the census covers {name}"
            fields = {f["name"] for f in schema["SubscriptionView"]["shape"]["fields"]}
            assert_eq(
                fields,
                {"subscription", "conn", "revision", "delivered_count", "armed"},
                "L: and states its exact key set",
            )
    finally:
        for c in clients:
            try:
                c.close()
            except OSError:
                pass
        try:
            sock_path.unlink()
        except FileNotFoundError:
            pass


if __name__ == "__main__":
    sys.exit(run_demo("R1552 §5.7 the server speaks first", body))
