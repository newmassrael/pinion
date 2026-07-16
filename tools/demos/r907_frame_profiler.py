#!/usr/bin/env python3
"""R907 §5.16 §5.7 — frame-timing profiler substrate end-to-end RPC
verification demo.

R907 lands the "measure" half of the §1 northern-star's measure-first
pro-tool-performance axis. `AppShell::render_window` now brackets every
painted frame into phases — **build** (`view` fn + layout), **encode**
(structured scene → vello fragments), **acquire** (the vsync block, split
out of render by R1361.1) and **render** (GPU command-buffer submit) —
plus the **total** productive frame span, and
feeds the sample into a per-window rolling window
(`pinion_runtime::FrameTimingStats`, capped at FRAME_TIMING_WINDOW =
120). `scene/frame_timings` projects it: the last frame's breakdown,
the window's min/mean/max + per-phase means, the cumulative
`frame_count`, and `mean_fps`.

The example `hello-frame-profiler` paints a small shaped-text workload
plus a Toggle that, with a benign input arc, drives one more measured
frame.

## The verification idea: deterministic invariants on wall-clock data

Frame timings are wall-clock — a demo can never assert "build took
320µs". But the *structure* is deterministic regardless of the
machine:

  - **phase partition**: total_us >= build + encode + acquire + render, and
    total == build + encode + acquire + render + other, EXACTLY — the
    four phases are disjoint sub-intervals of the total interval, and µs
    truncation preserves `Σ⌊subᵢ⌋ <= ⌊total⌋`.
  - **window ordering**: min_total <= mean_total <= max_total, and the
    last frame's total sits within [min, max].
  - **fps consistency**: mean_fps ≈ 1e6 / mean_total_us.
  - **window cap**: window_len == min(frame_count, 120).
  - **monotonic count**: frame_count strictly increases per driven
    repaint (idle window → only a driven arc advances it).
  - **unknown window**: a non-existent window id is rejected (-32602),
    distinct from the FrameTimingsUnavailable bootstrap state.

## Demo scope (>=30 assertions, sections A-H)

  (A) Wire shape — all top-level + nested keys present and typed.
  (B) Phase-partition invariant on the last frame.
  (C) Window ordering invariant.
  (D) mean_fps consistency with mean_total_us.
  (E) frame_count strictly increases per driven repaint.
  (F) window_len == min(frame_count, 120).
  (G) Invariants hold across a stress run of driven frames.
  (H) Unknown-window rejection (-32602).
  (I) Idle window paints NOTHING (the seam must not drive paint);
      a declared fps target streams without any input.
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
    run_demo,
    wait_until,
)

# The example's declared canonical viewport (WIN_W x WIN_H).
_WIN_W = 360
_WIN_H = 460

# Mirror of `pinion_runtime::FRAME_TIMING_WINDOW` — the rolling-window
# cap the substrate enforces; the demo asserts window_len never exceeds
# it and equals min(frame_count, cap).
_WINDOW_CAP = 120

# (I) R1361 — idle/stream observation window. 1s is long enough that a
# 60fps target lands ~60 frames and a spin lands hundreds, and short
# enough to keep the demo brisk.
_IDLE_OBSERVE_S = 1.0
_STREAM_FPS = 60
# Generous band: a loaded CI box paces slower, but "paints on its own"
# is unmistakable at >= 15, and a free-running (unpaced) loop would
# blow well past the ceiling.
_STREAM_FLOOR = 15
_STREAM_CEIL = 200

# Top-level + nested wire keys the typed FrameTimingsOutcome carries.
_LAST_KEYS = (
    "build_us",
    "encode_us",
    # R1361.1 — the swapchain-acquire block, split out of render_us.
    "acquire_us",
    "render_us",
    "total_us",
    "other_us",
    "work_us",
)
_WINDOW_KEYS = (
    "min_total_us",
    "mean_total_us",
    "max_total_us",
    "mean_build_us",
    "mean_acquire_us",
    "mean_encode_us",
    "mean_render_us",
)


def _drive_redraw(tf: RpcSubprocess) -> None:
    """Click the "repaint" Toggle so its state flips -> the view
    re-runs (paint_hash re-keys) -> AppShell::render_window runs ->
    record_frame_timing bumps frame_count. A minimal idle window (no
    hover-sensitive chrome) does not repaint on a benign no-op click,
    so the demo drives a real state change. Each click flips A<->B, so
    repeated calls keep producing distinct frames."""
    try:
        tf.request("scene/click", {"path": "repaint"})
    except Exception:  # noqa: BLE001
        pass


def _drive_frame_beyond(tf: RpcSubprocess, baseline: int, desc: str) -> None:
    """Drive redraw arcs until frame_count strictly exceeds `baseline`
    (R883 zero-flake). Real paint cycles fire only on winit's
    RedrawRequested, asynchronous to RPC dispatch — so the observed
    profiler counter, not wall-clock, is the gate."""
    def advanced() -> bool:
        try:
            fc = int(tf.frame_timings()["frame_count"])
        except RpcError:
            fc = -1
        if fc > baseline:
            return True
        _drive_redraw(tf)
        return False

    wait_until(advanced, desc=desc)


def _wait_available(tf: RpcSubprocess) -> dict[str, Any]:
    """Poll until the primary window has painted at least one frame and
    `scene/frame_timings` returns a snapshot (no longer
    FrameTimingsUnavailable). Returns the first snapshot."""
    def poll() -> Any:
        try:
            return tf.frame_timings()
        except RpcError:
            _drive_redraw(tf)
            return None

    return wait_until(poll, desc="scene/frame_timings becomes available")


def _assert_wire_shape(ft: dict[str, Any]) -> None:
    """(A) every documented key present + correctly typed."""
    for key in ("frame_count", "window_len", "last", "window", "mean_fps"):
        assert key in ft, f"scene/frame_timings result must carry '{key}'; got {ft!r}"
    assert isinstance(ft["frame_count"], int) and ft["frame_count"] >= 1, (
        f"frame_count must be a positive int once painted; got {ft['frame_count']!r}"
    )
    assert isinstance(ft["window_len"], int) and ft["window_len"] >= 1, (
        f"window_len must be a positive int; got {ft['window_len']!r}"
    )
    assert ft["window_len"] <= _WINDOW_CAP, (
        f"window_len must not exceed FRAME_TIMING_WINDOW={_WINDOW_CAP}; "
        f"got {ft['window_len']}"
    )
    last = ft["last"]
    assert isinstance(last, dict), f"'last' must be an object; got {last!r}"
    for key in _LAST_KEYS:
        assert key in last, f"last.{key} must be present; got {last!r}"
        assert isinstance(last[key], int) and last[key] >= 0, (
            f"last.{key} must be a non-negative int; got {last[key]!r}"
        )
    window = ft["window"]
    assert isinstance(window, dict), f"'window' must be an object; got {window!r}"
    for key in _WINDOW_KEYS:
        assert key in window, f"window.{key} must be present; got {window!r}"
        assert isinstance(window[key], int) and window[key] >= 0, (
            f"window.{key} must be a non-negative int; got {window[key]!r}"
        )
    assert isinstance(ft["mean_fps"], (int, float)) and ft["mean_fps"] >= 0.0, (
        f"mean_fps must be a non-negative number; got {ft['mean_fps']!r}"
    )


def _assert_partition(ft: dict[str, Any]) -> None:
    """(B) total >= build + encode + acquire + render, and the five-way
    partition total == phases + other holds exactly (R1361.1 added the
    acquire phase; before it, the vsync block was billed to render)."""
    last = ft["last"]
    phase_sum = (
        last["build_us"] + last["encode_us"] + last["acquire_us"] + last["render_us"]
    )
    assert last["total_us"] >= phase_sum, (
        f"total_us ({last['total_us']}) must be >= build+encode+acquire+render "
        f"({phase_sum}) — disjoint sub-intervals"
    )
    assert last["total_us"] == phase_sum + last["other_us"], (
        f"total must partition exactly into phases + other: "
        f"total={last['total_us']} phases={phase_sum} other={last['other_us']}"
    )


def _assert_work_excludes_the_acquire_block(ft: dict[str, Any]) -> None:
    """(B2) R1361.1 — work_us == build + encode + render, i.e. the frame
    minus the acquire BLOCK. This is the split's whole point: `total`
    answers "how long was the frame", `work` answers "how much of it was
    mine". They differ by exactly the vsync wait.

    Structural, never a magnitude: acquire may legitimately be 0 (nothing
    to wait for) or dominate (vsync-paced), and both are healthy."""
    last = ft["last"]
    expected = last["build_us"] + last["encode_us"] + last["render_us"]
    assert last["work_us"] == expected, (
        f"work_us ({last['work_us']}) must be build+encode+render "
        f"({expected}) — the acquire block is excluded by definition"
    )
    assert last["work_us"] <= last["total_us"], (
        f"work ({last['work_us']}) cannot exceed the frame ({last['total_us']})"
    )
    # total - work is the block plus post-paint overhead, never negative.
    assert last["total_us"] - last["work_us"] == last["acquire_us"] + last["other_us"], (
        f"the gap between total and work is exactly acquire+other: "
        f"total={last['total_us']} work={last['work_us']} "
        f"acquire={last['acquire_us']} other={last['other_us']}"
    )


def _assert_idle_window_paints_nothing(tf: RpcSubprocess) -> None:
    """(I) R1361 — the load-bearing property of the `use_frame_timings`
    seam, proven end-to-end: a HUD that READS the frame timings every
    paint must not CAUSE a paint.

    The binding's view fn calls `use_frame_timings()` on every frame. If
    that seam were a `Signal` (the obvious design), each publish would
    dirty the owner, the shell's dirty->redraw bridge would arm a repaint,
    and — because frame timings differ every frame by construction, so the
    equality-skip can never fire — an idle window would spin at 100% CPU
    forever. A measurement of painting must not drive painting.

    ZERO-FLAKE by margin, not by luck: the correct answer is exactly 0 and
    the failure mode is hundreds. There is no boundary to sit on.
    """
    before = int(tf.frame_timings()["frame_count"])
    time.sleep(_IDLE_OBSERVE_S)
    after = int(tf.frame_timings()["frame_count"])
    assert after == before, (
        f"an UNPACED window with no input must paint NOTHING in "
        f"{_IDLE_OBSERVE_S}s; frame_count went {before} -> {after} "
        f"(+{after - before}). A non-zero delta means something is driving "
        f"repaints on its own — the `use_frame_timings` seam becoming "
        f"reactive is the way this regresses"
    )


def _assert_declared_target_streams_without_input(tf: RpcSubprocess) -> None:
    """(I) R1361 — the peer of the above: declaring a frame target IS what
    makes frames stream. No clicks, just `scene/set_fps 60`; the window
    must then paint on its own at roughly the target.

    Asserts a generous band, never a pinned rate: the point is "it paces at
    all, near the target", and a loaded CI box will land low.
    """
    tf.request("scene/set_fps", {"fps": _STREAM_FPS})
    before = int(tf.frame_timings()["frame_count"])
    time.sleep(_IDLE_OBSERVE_S)
    painted = int(tf.frame_timings()["frame_count"]) - before
    assert painted >= _STREAM_FLOOR, (
        f"a {_STREAM_FPS}fps target must make the window paint on its own: "
        f"expected >= {_STREAM_FLOOR} frames in {_IDLE_OBSERVE_S}s, got "
        f"{painted}. Zero means the declared target is not driving the loop"
    )
    assert painted <= _STREAM_CEIL, (
        f"a {_STREAM_FPS}fps target must PACE, not free-run: got {painted} "
        f"frames in {_IDLE_OBSERVE_S}s (ceiling {_STREAM_CEIL})"
    )
    # …and the pacing is the same signal the profiler judges jank against.
    tf.request("scene/set_fps", {"fps": 0})


def _assert_window_ordering(ft: dict[str, Any]) -> None:
    """(C) min <= mean <= max over the window, last within [min, max]."""
    w = ft["window"]
    assert w["min_total_us"] <= w["mean_total_us"], (
        f"min ({w['min_total_us']}) must be <= mean ({w['mean_total_us']})"
    )
    assert w["mean_total_us"] <= w["max_total_us"], (
        f"mean ({w['mean_total_us']}) must be <= max ({w['max_total_us']})"
    )
    last_total = ft["last"]["total_us"]
    assert w["min_total_us"] <= last_total <= w["max_total_us"], (
        f"last total ({last_total}) must sit within "
        f"[{w['min_total_us']}, {w['max_total_us']}]"
    )


def _assert_fps_consistency(ft: dict[str, Any]) -> None:
    """(D) mean_fps == 1e6 / mean_total_us (0.0 for a zero mean)."""
    mean_total = ft["window"]["mean_total_us"]
    mean_fps = float(ft["mean_fps"])
    if mean_total == 0:
        assert abs(mean_fps) < 1e-6, (
            f"zero mean_total_us must report mean_fps 0.0; got {mean_fps}"
        )
        return
    expected = 1_000_000.0 / mean_total
    # f32 storage of a ~10^3..10^4 fps value: relative error ~1e-4. A
    # 0.1% + 0.5 absolute tolerance is comfortably above f32 rounding
    # and well below any meaningful divergence.
    tol = expected * 1e-3 + 0.5
    assert abs(mean_fps - expected) <= tol, (
        f"mean_fps ({mean_fps}) must invert mean_total_us ({mean_total}): "
        f"expected ~{expected:.3f}, tol {tol:.3f}"
    )


def _assert_phase_means_bounded(ft: dict[str, Any]) -> None:
    """Per-phase means also partition the mean total: each phase mean is
    <= mean_total, and their sum is <= mean_total (averaging preserves
    the per-frame partition, and `Σ⌊·⌋ <= ⌊Σ·⌋` survives the truncation
    — deterministic regardless of wall-clock magnitude)."""
    w = ft["window"]
    assert w["mean_build_us"] <= w["mean_total_us"], (
        f"mean_build ({w['mean_build_us']}) must be <= mean_total "
        f"({w['mean_total_us']})"
    )
    assert w["mean_encode_us"] <= w["mean_total_us"], (
        f"mean_encode ({w['mean_encode_us']}) must be <= mean_total "
        f"({w['mean_total_us']})"
    )
    assert w["mean_acquire_us"] <= w["mean_total_us"], (
        f"mean_acquire ({w['mean_acquire_us']}) must be <= mean_total "
        f"({w['mean_total_us']})"
    )
    assert w["mean_render_us"] <= w["mean_total_us"], (
        f"mean_render ({w['mean_render_us']}) must be <= mean_total "
        f"({w['mean_total_us']})"
    )
    phase_mean_sum = w["mean_build_us"] + w["mean_encode_us"] + w["mean_render_us"]
    assert phase_mean_sum <= w["mean_total_us"], (
        f"sum of phase means ({phase_mean_sum}) must be <= mean_total "
        f"({w['mean_total_us']})"
    )


def _assert_window_len_cap(ft: dict[str, Any]) -> None:
    """(F) window_len == min(frame_count, FRAME_TIMING_WINDOW)."""
    expected = min(int(ft["frame_count"]), _WINDOW_CAP)
    assert ft["window_len"] == expected, (
        f"window_len must equal min(frame_count, {_WINDOW_CAP}); "
        f"frame_count={ft['frame_count']} window_len={ft['window_len']} "
        f"expected={expected}"
    )


def body() -> None:
    with RpcSubprocess("hello-frame-profiler", boot_grace=1.5) as tf:
        # ── (A) Wire shape ───────────────────────────────────────
        ft_a = _wait_available(tf)
        _assert_wire_shape(ft_a)

        # ── (B) Phase-partition invariant ────────────────────────
        _assert_partition(ft_a)
        _assert_work_excludes_the_acquire_block(ft_a)

        # ── (C) Window ordering invariant ────────────────────────
        _assert_window_ordering(ft_a)

        # ── (D) mean_fps consistency + phase-mean partition ──────
        _assert_fps_consistency(ft_a)
        _assert_phase_means_bounded(ft_a)

        # ── (E) frame_count strictly increases per driven repaint ─
        start_count_e = int(ft_a["frame_count"])
        prev_count = start_count_e
        for i in range(3):
            _drive_frame_beyond(tf, prev_count, f"driven frame {i + 1}")
            ft_loop = tf.frame_timings()
            now_count = int(ft_loop["frame_count"])
            assert now_count > prev_count, (
                f"frame_count must strictly increase per driven repaint; "
                f"prev={prev_count} now={now_count}"
            )
            # Invariants must still hold on every fresh frame.
            _assert_partition(ft_loop)
            _assert_work_excludes_the_acquire_block(ft_loop)
            _assert_window_ordering(ft_loop)
            prev_count = now_count
        assert prev_count - start_count_e >= 3, (
            f"three driven frames must advance frame_count by >= 3; "
            f"start={start_count_e} end={prev_count}"
        )

        # ── (F) window_len == min(frame_count, cap) ──────────────
        _assert_window_len_cap(tf.frame_timings())

        # ── (G) Invariants hold across a stress run ──────────────
        start_count_g = prev_count
        last_count = prev_count
        for i in range(6):
            _drive_frame_beyond(tf, last_count, f"stress frame {i + 1}")
            ft_s = tf.frame_timings()
            _assert_wire_shape(ft_s)
            _assert_partition(ft_s)
            _assert_work_excludes_the_acquire_block(ft_s)
            _assert_window_ordering(ft_s)
            _assert_fps_consistency(ft_s)
            _assert_phase_means_bounded(ft_s)
            _assert_window_len_cap(ft_s)
            cur = int(ft_s["frame_count"])
            assert cur > last_count, (
                f"frame_count must keep advancing under the stress run; "
                f"prev={last_count} now={cur}"
            )
            last_count = cur
        assert last_count - start_count_g >= 6, (
            f"six stress frames must advance frame_count by >= 6; "
            f"start={start_count_g} end={last_count}"
        )

        # The default window ("main") and the explicit window id resolve
        # to the same primary snapshot.
        ft_main = tf.frame_timings(window="main")
        _assert_wire_shape(ft_main)
        assert int(ft_main["frame_count"]) >= last_count, (
            "explicit window='main' must address the same primary window "
            "as the default scope (count only advances)"
        )

        # ── (I) The seam must not drive paint; a target must ─────
        # R1361 — run BEFORE (H) so the window is left unpaced-and-idle
        # exactly as it started. `set_fps 0` inside the stream check
        # restores the no-target state (section A's contract).
        _assert_idle_window_paints_nothing(tf)
        _assert_declared_target_streams_without_input(tf)

        # ── (H) Unknown-window rejection ─────────────────────────
        # A non-existent window id is rejected at dispatch entry
        # (R889 unknown_window verdict, -32602) — categorically
        # different from FrameTimingsUnavailable (a known but
        # unpainted window).
        raised = False
        try:
            tf.frame_timings(window="ghost-window-does-not-exist")
        except RpcError as exc:
            raised = True
            assert exc.code == -32602, (
                f"unknown window must be -32602 Invalid params; got {exc.code}"
            )
        assert raised, "querying an unknown window id must raise, not succeed"


if __name__ == "__main__":
    sys.exit(run_demo("R907 frame-timing profiler substrate", body))
