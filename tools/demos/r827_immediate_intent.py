#!/usr/bin/env python3
"""R827 §2 #4 §5.20 — immediate-mode -> retained intent bridge RPC demo.

`hello-immediate-intent` is the first consumer of
`ImmediateMode::drain_intents` + `walk_scene_and_drain_immediate`: the
bidirectional half of the §2 #4 dual-execution model. R681 proved
retained -> immediate (a driver `tick`s/`paint`s every frame); this
binding proves immediate -> retained — the immediate-mode driver emits
§5.20 intents the retained `V::update` reducer consumes, so the game
loop drives retained app state.

The driver is a 1-D bouncing ball: every wall reflection buffers one
`bounce` intent. The substrate, immediately after
`Scene::tick_immediate_mode`, drains those intents (prefixing the node
tag -> `ball.bounce`, R22) and routes each through `V::update`, which
increments a reactive `Signal<u32>`. The view fn renders the count as
`"Bounces: N"`, so an AI client watches retained state climb purely
through `scene/snapshot` — the §2 #2 + §2 #7 scene-as-data invariant
holds across the retained / immediate boundary.

Verification scope (>= 30 assertions):

  (A) Substrate sanity — boot; the paint scene carries the retained
      Button trigger, the bounce readout text, AND the immediate-mode
      ball node, each addressable by §5.20 tag; the node's post-layout
      viewport is non-zero.

  (B) The bridge — the retained `Bounces:` count starts at zero and
      climbs as the continuous game loop bounces the ball, observed
      entirely through scene/snapshot (the genuinely-new R827
      capability). Monotonic non-decreasing across samples.

  (C) Retained pointer routing survives the composition — scene/click
      on the Dismiss Button transitions its SCXML state; the bounce
      loop keeps running underneath.

  (D) Keyboard activation arc survives — focus + scene/key "Space" on
      the Button leaves it addressable (the immediate subtree does not
      absorb keyboard input).

  (E) ARIA Button role — verified by the binding's own unit test
      `r827_access_node_is_aria_button`.

  (F) Continuous paint clock — repeated snapshots over a wall-clock
      window stay non-degenerate; the ball node survives every frame.

  (G) Composite paint-root tag (R55.G.17).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    run_demo,
    wait_paint_beyond,
    wait_until,
)

_WIN_W = 340
_WIN_H = 360

_BALL_NODE_TAG = "ball"
_DISMISS_BTN_TAG = "dismiss_btn"
_COUNT_READOUT_TAG = "bounce_readout"
_VIEW_TAG = "ball_view"


def _snap(tf: RpcSubprocess) -> dict:
    """Read the binding's paint scene at the canonical viewport."""
    resp = tf.snapshot("", source="paint", viewport=(_WIN_W, _WIN_H))
    assert isinstance(resp, dict) and resp, "scene/snapshot must return a paint scene"
    return resp


def _walk_immediate_node(scene: Any) -> dict | None:
    """First ImmediateModeNode dict in the paint scene (DFS pre-order)."""
    if not isinstance(scene, dict):
        return None
    if scene.get("type") == "ImmediateModeNode":
        return scene
    children = scene.get("children")
    if isinstance(children, list):
        for child in children:
            found = _walk_immediate_node(child)
            if found is not None:
                return found
    content = scene.get("content")
    if isinstance(content, dict):
        return _walk_immediate_node(content)
    return None


def _read_bounce_count(tf: RpcSubprocess) -> int:
    """Parse the retained `Bounces: N` readout text from a fresh paint
    snapshot. Raises AssertionError if the readout is missing or
    malformed — the bridge's observable must always be present."""
    snap = _snap(tf)
    node = find_by_tag(snap, _COUNT_READOUT_TAG)
    assert node is not None, f"{_COUNT_READOUT_TAG!r} text node must be addressable"
    content = node.get("content")
    assert isinstance(content, str), f"readout content must be a string; got {content!r}"
    assert content.startswith("Bounces: "), f"unexpected readout format: {content!r}"
    return int(content.removeprefix("Bounces: "))


def body() -> None:
    # Immediate-mode bindings need the game loop running before the first
    # observation; a generous boot grace lets the WaitUntil pacing arm.
    with RpcSubprocess("hello-immediate-intent", boot_grace=1.5) as tf:
        # ── (A) Substrate sanity ─────────────────────────────────────
        snap_a = _snap(tf)
        assert find_by_tag(snap_a, _DISMISS_BTN_TAG) is not None, (
            "Dismiss Button (retained side of the §2 #4 composition) "
            "must be addressable by its §5.20 tag"
        )
        assert find_by_tag(snap_a, _BALL_NODE_TAG) is not None, (
            "ball node (immediate-mode subtree opt-in) must be addressable"
        )
        assert find_by_tag(snap_a, _COUNT_READOUT_TAG) is not None, (
            "bounce readout text must be addressable"
        )
        imm = _walk_immediate_node(snap_a)
        assert imm is not None, "paint scene must surface a Scene::ImmediateModeNode"
        assert imm.get("tag") == _BALL_NODE_TAG, (
            "DFS walker must reach the same node the tag walker resolved"
        )
        viewport = imm.get("viewport") or imm.get("rect")
        assert isinstance(viewport, dict), f"node must expose a viewport; got {imm!r}"
        assert viewport.get("w", 0) > 0 and viewport.get("h", 0) > 0, (
            f"post-layout viewport must be non-zero; got {viewport!r}"
        )

        # ── (B) The bridge: retained count climbs from the game loop ──
        count0 = _read_bounce_count(tf)
        assert count0 >= 0, f"initial bounce count must be non-negative; got {count0}"

        # Wait for the continuous game loop to bounce the ball at least
        # once and the reducer to register it in retained state. The ball
        # crosses the full track (~0.4s) repeatedly, so a bounce lands
        # well within the wait_until timeout.
        first = wait_until(
            lambda: (_read_bounce_count(tf) > count0),
            timeout=8.0,
            interval=0.05,
            desc="retained bounce count climbs past initial via immediate->retained intent",
        )
        assert first is True
        count1 = _read_bounce_count(tf)
        assert count1 > count0, (
            f"immediate-mode bounce intent must increment retained state: "
            f"{count1} !> {count0}"
        )
        assert count1 >= count0 + 1

        # Monotonic non-decreasing: a later sample is never below an
        # earlier one (the counter only grows).
        count_mid = _read_bounce_count(tf)
        assert count_mid >= count1, f"count must not regress: {count_mid} < {count1}"

        # Cross a higher threshold to prove sustained bridging, not a
        # one-off — several bounces route through V::update over time.
        target = count1 + 3
        wait_until(
            lambda: (_read_bounce_count(tf) >= target),
            timeout=10.0,
            interval=0.05,
            desc=f"retained bounce count reaches {target} (sustained bridging)",
        )
        count2 = _read_bounce_count(tf)
        assert count2 >= target, f"sustained bridge: {count2} < {target}"
        assert count2 >= count_mid

        # The ball node + readout survive the bridging window.
        snap_b = _snap(tf)
        assert _walk_immediate_node(snap_b) is not None
        assert find_by_tag(snap_b, _COUNT_READOUT_TAG) is not None

        # ── (C) Retained pointer routing survives the composition ────
        state_before = tf.query("/external/state")
        assert state_before in ("Idle", "Hover"), (
            f"Button starts Idle/Hover; got {state_before!r}"
        )
        tf.click(path=_DISMISS_BTN_TAG)
        state_after = tf.query("/external/state")
        # A full press+release Material click resolves to Hover (cursor
        # remains on the button after release).
        assert state_after in ("Hover", "Pressed", "Idle"), (
            f"Button SCXML state must resolve after click; got {state_after!r}"
        )
        snap_c = _snap(tf)
        assert find_by_tag(snap_c, _DISMISS_BTN_TAG) is not None, (
            "Button must remain addressable after click"
        )
        # The bounce loop is unaffected by the retained click.
        count_after_click = _read_bounce_count(tf)
        wait_until(
            lambda: (_read_bounce_count(tf) > count_after_click),
            timeout=8.0,
            interval=0.05,
            desc="game loop keeps bridging bounces after a retained click",
        )
        assert _read_bounce_count(tf) > count_after_click

        # ── (D) Keyboard activation arc survives ─────────────────────
        tf.request("focus/set", {"tag": _DISMISS_BTN_TAG})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == _DISMISS_BTN_TAG,
            desc="dismiss button owns focus",
        )
        tf.key(path=_DISMISS_BTN_TAG, name="Space")
        snap_d = _snap(tf)
        assert find_by_tag(snap_d, _DISMISS_BTN_TAG) is not None, (
            "Button must survive a Space keypress"
        )
        assert _walk_immediate_node(snap_d) is not None, (
            "ball node must survive keyboard input (immediate subtree "
            "does not absorb keys)"
        )

        # ── (F) Continuous paint clock without input ─────────────────
        for _ in range(3):
            # Observed-state gate on the continuous paint clock: a real
            # frame must land between samples (R883 zero-flake).
            wait_paint_beyond(
                tf, int(tf.cache_stats()["paint_count"]),
                desc="continuous paint clock advanced a frame",
            )
            snap_f = _snap(tf)
            assert isinstance(snap_f, dict) and snap_f
            assert find_by_tag(snap_f, _DISMISS_BTN_TAG) is not None
            assert _walk_immediate_node(snap_f) is not None, (
                "ImmediateModeNode must survive every paint cycle"
            )

        # ── (G) Composite paint-root tag (R55.G.17) ──────────────────
        assert find_by_tag(snap_a, _DISMISS_BTN_TAG) is not None
        assert find_by_tag(snap_a, _VIEW_TAG) is not None, (
            "Root container must carry its tag for {path: V::tag()} routing"
        )


if __name__ == "__main__":
    sys.exit(run_demo("hello-immediate-intent R827 §2 #4 §5.20 intent bridge", body))
