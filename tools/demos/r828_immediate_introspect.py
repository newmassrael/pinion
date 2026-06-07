#!/usr/bin/env python3
"""R828 §2 #4 §5.12 — immediate-mode driver introspection RPC demo.

The read peer of R827's intent bridge. R827 gave the immediate-mode
driver an *output* channel (emit §5.20 intents -> retained reducer);
R828 gives it a *queryable state* channel: `BouncingBallDriver` opts in
to `ImmediateMode::introspect` (the channel R681 declared but left with
zero consumers — "atomic 4"), exposing `pos` / `velocity` / `bounces`.
`scene/query "/ball/external/<field>"` now reaches it through the same
§5.15 item 8 surface AI uses against `Scene::External` widgets.

The seam this closes: `ImmediateModeNode` holds its driver behind
`Rc<RefCell<dyn ImmediateMode>>` (only transiently borrowable) AND lives
only in the per-frame *paint* scene (absent from the boot-frozen state
scene). So query (a) resolves the value within the RefCell borrow scope
(it returns an owned value, never a borrow) and (b) falls back from the
state scene to the last painted scene on `NoExternalAtPath`. The paint
scene's `ImmediateModeNode.handle` is the same `Owner::cache` driver the
live game loop ticks, so the query reads current simulation state.

Verification scope (>= 30 assertions):

  (A) Substrate sanity — boot; ball node + bounce readout addressable.

  (B) `$schema` discovery — `scene/query "/ball/external/$schema"`
      returns the driver's declared {pos, velocity, bounces} contract
      (R825 discovery extends across the retained / immediate boundary).

  (C) Field reads — pos in [0,1], |velocity| ~ ball speed, bounces int.

  (D) Live state — querying `bounces` over a wall-clock window shows the
      driver's simulation tally climb (monotonic), read straight from
      the live driver Rc through the paint-scene fallback.

  (E) Cross-check — the driver's own `bounces` (R828 read channel) and
      the retained `Bounces: N` readout (R827 write bridge counting
      `ball.bounce` intents) track the same reflection events, so they
      stay close: both count every wall bounce, from opposite ends.

  (F) Error surface — an unknown introspect path is rejected.

  (G) Composition intact — the retained Dismiss Button still routes.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    find_by_tag,
    run_demo,
    wait_until,
)

_WIN_W = 340
_WIN_H = 360
_BALL_NODE_TAG = "ball"
_DISMISS_BTN_TAG = "dismiss_btn"
_COUNT_READOUT_TAG = "bounce_readout"
_BALL_SPEED = 2.5


def _read_retained_count(tf: RpcSubprocess) -> int:
    """Parse the retained `Bounces: N` readout (R827 intent bridge)."""
    snap = tf.snapshot("", source="paint", viewport=(_WIN_W, _WIN_H))
    node = find_by_tag(snap, _COUNT_READOUT_TAG)
    assert node is not None, "bounce readout text must be addressable"
    content = node.get("content")
    assert isinstance(content, str) and content.startswith("Bounces: "), (
        f"unexpected readout format: {content!r}"
    )
    return int(content.removeprefix("Bounces: "))


def _query_bounces(tf: RpcSubprocess) -> int:
    val = tf.query(f"/{_BALL_NODE_TAG}/external/bounces")
    assert isinstance(val, int), f"bounces must be an int; got {val!r}"
    return val


def body() -> None:
    with RpcSubprocess("hello-immediate-intent", boot_grace=1.5) as tf:
        # ── (A) Substrate sanity ─────────────────────────────────────
        snap = tf.snapshot("", source="paint", viewport=(_WIN_W, _WIN_H))
        assert find_by_tag(snap, _BALL_NODE_TAG) is not None, "ball node addressable"
        assert find_by_tag(snap, _COUNT_READOUT_TAG) is not None, "readout addressable"

        # ── (B) $schema discovery on the immediate-mode driver ───────
        schema = tf.query(f"/{_BALL_NODE_TAG}/external/$schema")
        assert isinstance(schema, list), f"$schema must be a JSON array; got {schema!r}"
        paths = {f.get("path") for f in schema if isinstance(f, dict)}
        assert {"pos", "velocity", "bounces"} <= paths, (
            f"driver schema must declare pos/velocity/bounces; got {paths}"
        )
        types = {f["path"]: f["type"] for f in schema if isinstance(f, dict)}
        assert types.get("pos") == "float"
        assert types.get("velocity") == "float"
        assert types.get("bounces") == "int"

        # ── (C) Field reads ──────────────────────────────────────────
        pos = tf.query(f"/{_BALL_NODE_TAG}/external/pos")
        assert isinstance(pos, (int, float)), f"pos must be numeric; got {pos!r}"
        assert 0.0 <= float(pos) <= 1.0, f"pos must be in [0,1]; got {pos}"
        vel = tf.query(f"/{_BALL_NODE_TAG}/external/velocity")
        assert isinstance(vel, (int, float)), f"velocity must be numeric; got {vel!r}"
        assert abs(abs(float(vel)) - _BALL_SPEED) < 0.01, (
            f"|velocity| must equal ball speed {_BALL_SPEED}; got {vel}"
        )
        b0 = _query_bounces(tf)
        assert b0 >= 0, f"bounces must be non-negative; got {b0}"

        # ── (D) Live driver state climbs via the paint-scene fallback ─
        wait_until(
            lambda: (_query_bounces(tf) > b0),
            timeout=8.0,
            interval=0.05,
            desc="driver bounces tally climbs (read live through introspect)",
        )
        b1 = _query_bounces(tf)
        assert b1 > b0, f"driver bounces must climb: {b1} !> {b0}"
        b_mid = _query_bounces(tf)
        assert b_mid >= b1, f"bounces must not regress: {b_mid} < {b1}"
        target = b1 + 3
        wait_until(
            lambda: (_query_bounces(tf) >= target),
            timeout=10.0,
            interval=0.05,
            desc=f"driver bounces reach {target}",
        )
        assert _query_bounces(tf) >= target

        # pos stays bounded across the window.
        pos2 = tf.query(f"/{_BALL_NODE_TAG}/external/pos")
        assert 0.0 <= float(pos2) <= 1.0, f"pos stays in [0,1]; got {pos2}"

        # ── (E) Cross-check: read channel vs write bridge converge ───
        # The driver's own `bounces` (R828, counted at the reflection)
        # and the retained readout (R827, counted at intent drain) track
        # the same events from opposite ends. Read-skew + the one-frame
        # reducer lag keep them within a small window, never far apart.
        driver_b = _query_bounces(tf)
        retained_b = _read_retained_count(tf)
        assert retained_b > 0, f"retained count must have climbed; got {retained_b}"
        assert abs(driver_b - retained_b) <= 5, (
            f"driver introspect ({driver_b}) and retained bridge ({retained_b}) "
            f"must track the same bounce events"
        )

        # ── (F) Error surface — unknown introspect path rejected ─────
        rejected = False
        try:
            tf.query(f"/{_BALL_NODE_TAG}/external/ghost")
        except RpcError:
            rejected = True
        assert rejected, "unknown introspect path must be rejected"

        # ── (G) Composition intact — retained Button still routes ────
        state_before = tf.query("/external/state")
        assert state_before in ("Idle", "Hover"), f"button state {state_before!r}"
        tf.click(path=_DISMISS_BTN_TAG)
        time.sleep(0.1)
        snap_g = tf.snapshot("", source="paint", viewport=(_WIN_W, _WIN_H))
        assert find_by_tag(snap_g, _DISMISS_BTN_TAG) is not None
        # The driver keeps simulating underneath the retained click.
        after = _query_bounces(tf)
        wait_until(
            lambda: (_query_bounces(tf) > after),
            timeout=8.0,
            interval=0.05,
            desc="driver keeps simulating after a retained click",
        )
        assert _query_bounces(tf) > after


if __name__ == "__main__":
    sys.exit(run_demo("hello-immediate-intent R828 §2 #4 §5.12 driver introspect", body))
