#!/usr/bin/env python3
"""R830 §2 #4 §5.15 — immediate-mode pointer input forwarding.

The player -> game input channel, completing the §2 #4 immediate-mode
I/O surface: time (R681 tick), output (R681 paint), game -> app (R827
intents), observe (R828 introspect), AI clock control (R829 stepping),
and now PLAYER -> GAME here. A pointer press inside an `ImmediateModeNode`
viewport is forwarded to the driver's `ImmediateMode::on_pointer_down` in
viewport-local logical pixels — the same space the driver paints in.

The retained input core dispatches to `Scene::External` widgets in the
state scene; immediate drivers are paint-scene-only and not `External`,
so the shell bridges the router's resolved hit-target to the driver
(`find_immediate_with_tag` on the last paint scene) without touching the
router. `BouncingBallDriver::on_pointer_down` reverses the ball's travel
direction and records the press point.

This demo composes the prior rounds: it PAUSES with `scene/set_fps 0`
(R829) so the click test is deterministic (a paused sim clock is frozen
— an incidental repaint does NOT advance it, so the click's direction
flip is the ONLY change), and reads the result through `scene/query`
(R828). Verification scope >= 30 assertions:

  (A) Pre-click state — `clicked` false, `last_click_x` null.
  (B) Click reverses direction — paused, click the ball: velocity flips
      sign exactly (-v0), pos is unchanged (frozen), `clicked` true, and
      `last_click_{x,y}` equals the viewport-local press point (~center).
  (C) Toggle — a second click flips velocity back to +v0.
  (D) Isolation — clicking the retained Dismiss Button does NOT forward
      to the ball driver (its velocity is untouched).
  (E) Resume — `set_fps 60` restarts the loop; the ball bounces again.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    run_demo,
    wait_until,
)

_WIN_W = 340
_WIN_H = 360
_BALL = "ball"
_BTN = "dismiss_btn"


def _vel(tf: RpcSubprocess) -> float:
    return float(tf.query(f"/{_BALL}/external/velocity"))


def _pos(tf: RpcSubprocess) -> float:
    return float(tf.query(f"/{_BALL}/external/pos"))


def _clicked(tf: RpcSubprocess) -> bool:
    return bool(tf.query(f"/{_BALL}/external/clicked"))


def _retained_clicks(tf: RpcSubprocess) -> int:
    """Parse the retained `Clicks: N` readout — the reducer's count of
    `ball.click` intents drained from the driver. Proves the R830 paused
    intent-drain path (a click on a paused window still routes its intent
    to V::update)."""
    snap = tf.snapshot("", source="paint", viewport=(_WIN_W, _WIN_H))
    node = find_by_tag(snap, "click_readout")
    assert node is not None, "click readout addressable"
    content = node.get("content")
    assert isinstance(content, str) and content.startswith("Clicks: "), (
        f"unexpected click readout: {content!r}"
    )
    return int(content.removeprefix("Clicks: "))


def _ball_viewport(snap: Any) -> tuple[int, int]:
    node = find_by_tag(snap, _BALL)
    assert node is not None, "ball node addressable"
    rect = node.get("viewport") or node.get("rect")
    assert isinstance(rect, dict), f"ball node must expose a viewport; got {node!r}"
    return int(rect["w"]), int(rect["h"])


def body() -> None:
    with RpcSubprocess("hello-immediate-intent", boot_grace=1.5) as tf:
        snap = tf.snapshot("", source="paint", viewport=(_WIN_W, _WIN_H))
        vw, vh = _ball_viewport(snap)
        assert vw > 0 and vh > 0

        # ── (A) Pre-click introspect state ───────────────────────────
        assert tf.query(f"/{_BALL}/external/clicked") is False, "no click yet"
        assert tf.query(f"/{_BALL}/external/last_click_x") is None, "null before first press"

        # Pause so the click test is deterministic (R829): a frozen sim
        # clock means the click's direction flip is the ONLY change.
        tf.set_fps(0)

        def _frozen() -> bool:
            a = (_pos(tf), _vel(tf))
            # wall-clock semantic: frozen-clock proof needs two samples
            # spaced by a real-time window (see r829); the outer
            # wait_until retries until the pair stabilises.
            time.sleep(0.12)
            return a == (_pos(tf), _vel(tf))
        wait_until(_frozen, timeout=6.0, interval=0.05, desc="sim clock freezes after set_fps(0)")

        # ── (B) Click reverses direction + forwards coordinates ──────
        clicks0 = _retained_clicks(tf)
        v0 = _vel(tf)
        p0 = _pos(tf)
        tf.click(path=_BALL)
        wait_until(lambda: _clicked(tf), timeout=6.0, interval=0.04,
                   desc="click reaches the immediate driver (on_pointer_down)")
        # R830 — the click's `ball.click` intent must route to the reducer
        # EVEN THOUGH the window is paused (the intent drain is decoupled
        # from whether the sim advanced). The retained count climbs.
        wait_until(lambda: _retained_clicks(tf) == clicks0 + 1, timeout=6.0, interval=0.04,
                   desc="paused click intent routes to the retained reducer (R830)")
        assert _retained_clicks(tf) == clicks0 + 1
        v1 = _vel(tf)
        assert abs(v1 + v0) < 1e-4, f"click must reverse velocity: {v1} != -{v0}"
        # Frozen: the click does not advance the sim (only flips direction).
        assert abs(_pos(tf) - p0) < 1e-6, "paused click must not advance pos"
        assert _clicked(tf) is True
        # Forwarded coordinates: a path-click lands on the node centre, so
        # the viewport-local press point is ~ (vw/2, vh/2).
        lx = float(tf.query(f"/{_BALL}/external/last_click_x"))
        ly = float(tf.query(f"/{_BALL}/external/last_click_y"))
        assert abs(lx - vw / 2.0) <= 3.0, f"last_click_x {lx} != ~{vw / 2.0} (viewport-local centre)"
        assert abs(ly - vh / 2.0) <= 3.0, f"last_click_y {ly} != ~{vh / 2.0}"
        assert 0.0 <= lx <= float(vw), "press point inside the viewport"

        # ── (C) Second click toggles direction back ──────────────────
        tf.click(path=_BALL)
        wait_until(lambda: abs(_vel(tf) - v0) < 1e-4, timeout=6.0, interval=0.04,
                   desc="second click flips velocity back to +v0")
        assert abs(_vel(tf) - v0) < 1e-4

        # ── (D) Retained Button click does NOT forward to the driver ─
        vel_before_btn = _vel(tf)
        pos_before_btn = _pos(tf)
        clicks_before_btn = _retained_clicks(tf)
        tf.click(path=_BTN)
        # No-change verification on a frozen clock: the click dispatch
        # commits before the RPC response, so plain reads follow.
        assert abs(_vel(tf) - vel_before_btn) < 1e-6, (
            "clicking the retained Button must not reach the ball driver"
        )
        assert abs(_pos(tf) - pos_before_btn) < 1e-6, "button click leaves the frozen ball put"
        assert _retained_clicks(tf) == clicks_before_btn, (
            "a retained Button click must not emit a ball.click intent"
        )

        # ── (E) Resume — continuous loop restarts ────────────────────
        # `bounces` climbs again once the wall-clock loop runs.
        b_resume = int(tf.query(f"/{_BALL}/external/bounces"))
        tf.set_fps(60)
        wait_until(lambda: int(tf.query(f"/{_BALL}/external/bounces")) > b_resume,
                   timeout=8.0, interval=0.05, desc="ball resumes bouncing after set_fps(60)")
        assert int(tf.query(f"/{_BALL}/external/bounces")) > b_resume


if __name__ == "__main__":
    sys.exit(run_demo("hello-immediate-intent R830 §2 #4 §5.15 pointer input", body))
