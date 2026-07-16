#!/usr/bin/env python3
"""R925 §5.16 §5.7 — frame-budget compliance / jank end-to-end RPC demo.

R907 landed the "measure" half of the §1 northern-star's measure-first
pro-tool-performance axis: `scene/frame_timings` reports each frame's
build/encode/acquire/render breakdown plus the rolling window's min/mean/max and
mean fps. But "how long are frames taking?" is only half the pro-tool
question — the other half is "are they hitting the target?". R925 wires
the existing per-window frame *budget* (`frame_pacing::frame_budget_for_window`,
R681 — the deadline the render loop already paces to) into the profiler
projection, so `scene/frame_timings` now also answers, against the
declared `scene/set_fps` target:

  - `budget_us`            — the per-frame budget (`1e6 / target_fps`),
                             **omitted** when the window is unpaced.
  - `over_budget_frames`   — window samples with `total_us > budget_us`
                             (the dropped-frame / jank count — Unreal
                             `stat unit` hitches, Chrome janky frames).
  - `worst_overrun_us`     — the worst single hitch's magnitude.
  - `jank_ratio`           — `over_budget_frames / window_len` in [0, 1].

This is the measure-first signal a frame-budget tuning round reads before
it optimizes anything.

## The verification idea: deterministic invariants on wall-clock data

Frame timings are wall-clock — a demo can never assert "this frame
janked". But the *structure* is deterministic on any machine:

  - **budget is a pure function of the target**: `set_fps(N)` →
    `budget_us == ⌊1e6 / N⌋` (the same value the render loop paces to);
    `set_fps(0)` (paused) and "no target" → `budget_us` absent.
  - **partition**: `0 <= over_budget_frames <= window_len`.
  - **ratio**: `jank_ratio == over_budget_frames / window_len`, in [0, 1].
  - **worst-overrun ⟺ max_total**: with a budget, if any frame is over,
    `worst_overrun_us == max_total_us - budget_us` and `max_total_us >
    budget_us`; if none is over, both `worst_overrun_us == 0` and
    `max_total_us <= budget_us`. This ties the jank magnitude to the
    already-reported window max — no wall-clock assumption.
  - **huge budget ⇒ no jank**: a 1-fps target (a 1-second budget) is
    not exceeded by any healthy frame, so `over_budget_frames == 0`
    deterministically (a >1s frame would be a real defect, not flake —
    the same healthy-frame assumption `clamp_frame_dt` makes at 33ms).

## Demo scope (>=30 assertions, sections A-F)

  (A) Unpaced window — budget_us omitted, jank fields zero.
  (B) 1-fps target (1s budget) — budget present, zero jank, invariants.
  (C) 120-fps target (tight budget) — structural invariants on real,
      possibly-janky frames; worst-overrun ⟺ max_total either way.
  (D) Clearing the target restores the unpaced (no-budget) shape.
  (E) jank_ratio re-derives from the integer counts.
  (F) Unknown-window rejection on both the write and the read.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    run_demo,
    wait_until,
)

# 1-fps target → a 1-second budget = 1_000_000µs (Duration::from_secs_f64
# (1.0).as_micros()). No healthy frame of an 8-row text workload comes
# close, so over_budget_frames == 0 is deterministic here.
_ONE_FPS_BUDGET_US = 1_000_000
# 120-fps target → ⌊1e6 / 120⌋ via Duration nanos truncation = 8_333µs
# (matches the embedder's frame_budget_for_window path exactly; pinned by
# the pinion-shell `r925_*higher_fps_is_tighter` integration test).
_AT_120_BUDGET_US = 8_333

_JANK_KEYS = ("over_budget_frames", "worst_overrun_us", "jank_ratio")


def _drive_redraw(tf: RpcSubprocess) -> None:
    """Flip the "repaint" Toggle so the view re-runs and one more frame
    is recorded (the R907 driver — a benign no-op click would not
    repaint an idle window, so the state change is what drives paint)."""
    try:
        tf.request("scene/click", {"path": "repaint"})
    except Exception:  # noqa: BLE001
        pass


def _drive_frame_beyond(tf: RpcSubprocess, baseline: int, desc: str) -> None:
    """Drive redraw arcs until frame_count strictly exceeds `baseline`
    (R883 zero-flake: the observed profiler counter, never wall-clock, is
    the gate — real paints fire on winit's RedrawRequested, async to
    RPC)."""

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
    """Poll until the window has painted and `scene/frame_timings`
    returns a snapshot (no longer FrameTimingsUnavailable)."""

    def poll() -> Any:
        try:
            return tf.frame_timings()
        except RpcError:
            _drive_redraw(tf)
            return None

    return wait_until(poll, desc="scene/frame_timings becomes available")


def _assert_jank_fields_present(ft: dict[str, Any]) -> None:
    """The three jank counters are ALWAYS present and correctly typed
    (only budget_us is omitted when unpaced)."""
    assert isinstance(ft["over_budget_frames"], int) and ft["over_budget_frames"] >= 0, (
        f"over_budget_frames must be a non-negative int; got {ft.get('over_budget_frames')!r}"
    )
    assert isinstance(ft["worst_overrun_us"], int) and ft["worst_overrun_us"] >= 0, (
        f"worst_overrun_us must be a non-negative int; got {ft.get('worst_overrun_us')!r}"
    )
    assert isinstance(ft["jank_ratio"], (int, float)) and ft["jank_ratio"] >= 0.0, (
        f"jank_ratio must be a non-negative number; got {ft.get('jank_ratio')!r}"
    )


def _assert_unpaced_shape(ft: dict[str, Any]) -> None:
    """No declared target → budget_us omitted, every jank field neutral."""
    _assert_jank_fields_present(ft)
    assert "budget_us" not in ft or ft["budget_us"] is None, (
        f"budget_us must be omitted/null on an unpaced window; got {ft!r}"
    )
    assert ft["over_budget_frames"] == 0, (
        f"no budget ⇒ no over-budget frames; got {ft['over_budget_frames']}"
    )
    assert ft["worst_overrun_us"] == 0, (
        f"no budget ⇒ zero overrun; got {ft['worst_overrun_us']}"
    )
    assert abs(float(ft["jank_ratio"])) < 1e-6, (
        f"no budget ⇒ jank_ratio 0.0; got {ft['jank_ratio']}"
    )


def _assert_budget_invariants(ft: dict[str, Any], expected_budget_us: int) -> None:
    """The budget-relative invariants that hold for ANY wall-clock data
    once a target is declared."""
    _assert_jank_fields_present(ft)
    assert ft.get("budget_us") == expected_budget_us, (
        f"budget_us must equal the declared target's 1/fps "
        f"(expected {expected_budget_us}); got {ft.get('budget_us')!r}"
    )
    over = int(ft["over_budget_frames"])
    window_len = int(ft["window_len"])
    # (partition) 0 <= over <= window_len.
    assert 0 <= over <= window_len, (
        f"over_budget_frames ({over}) must lie in [0, window_len={window_len}]"
    )
    # (ratio) jank_ratio == over / window_len, in [0, 1].
    expected_ratio = over / window_len if window_len else 0.0
    assert abs(float(ft["jank_ratio"]) - expected_ratio) <= 1e-6, (
        f"jank_ratio ({ft['jank_ratio']}) must equal over/window_len "
        f"({over}/{window_len} = {expected_ratio:.6f})"
    )
    assert 0.0 <= float(ft["jank_ratio"]) <= 1.0, (
        f"jank_ratio must be a fraction in [0, 1]; got {ft['jank_ratio']}"
    )
    # (worst-overrun ⟺ max_total) ties the jank magnitude to the
    # already-reported window max — no wall-clock assumption.
    max_total = int(ft["window"]["max_total_us"])
    worst = int(ft["worst_overrun_us"])
    budget = int(ft["budget_us"])
    if over > 0:
        assert max_total > budget, (
            f"over_budget_frames>0 implies max_total ({max_total}) > budget ({budget})"
        )
        assert worst == max_total - budget, (
            f"worst_overrun_us ({worst}) must be max_total - budget "
            f"({max_total} - {budget} = {max_total - budget})"
        )
    else:
        assert max_total <= budget, (
            f"over_budget_frames==0 implies max_total ({max_total}) <= budget ({budget})"
        )
        assert worst == 0, f"no over-budget frame ⇒ worst_overrun_us 0; got {worst}"


def body() -> None:
    with RpcSubprocess("hello-frame-profiler", boot_grace=1.5) as tf:
        # ── (A) Unpaced window: no target declared yet ───────────────
        ft_a = _wait_available(tf)
        _assert_unpaced_shape(ft_a)

        # ── (B) 1-fps target → a 1-second budget, zero jank ──────────
        # The override opts the retained window into polled 1/fps pacing
        # (R681), so the reported budget IS the render loop's deadline.
        tf.set_fps(1)
        # Drive several fresh frames so the window is judged against the
        # new budget on real samples.
        base_b = int(tf.frame_timings()["frame_count"])
        for i in range(3):
            _drive_frame_beyond(tf, base_b + i, f"(B) driven frame {i + 1}")
        ft_b = tf.frame_timings()
        _assert_budget_invariants(ft_b, _ONE_FPS_BUDGET_US)
        # A 1-second budget is not exceeded by any healthy frame.
        assert ft_b["over_budget_frames"] == 0, (
            f"a 1s budget must see zero jank on healthy frames; "
            f"got {ft_b['over_budget_frames']} over of {ft_b['window_len']} "
            f"(max_total={ft_b['window']['max_total_us']}µs)"
        )
        assert ft_b["jank_ratio"] == 0.0 or abs(float(ft_b["jank_ratio"])) < 1e-6
        assert ft_b["worst_overrun_us"] == 0

        # ── (C) 120-fps target → tight budget, structural invariants ──
        # Real frames may or may not exceed an 8.3ms budget; the demo
        # asserts only what is deterministic regardless of magnitude.
        tf.set_fps(120)
        base_c = int(tf.frame_timings()["frame_count"])
        for i in range(3):
            _drive_frame_beyond(tf, base_c + i, f"(C) driven frame {i + 1}")
        ft_c = tf.frame_timings()
        _assert_budget_invariants(ft_c, _AT_120_BUDGET_US)
        # A tighter budget can only classify >= as many frames over than
        # a looser one — but that compares windows across time, so the
        # demo asserts the within-snapshot relations only (above).

        # ── (D) Clearing the target restores the unpaced shape ───────
        tf.set_fps(None)
        # Drive one fresh frame so the read reflects the cleared target.
        base_d = int(tf.frame_timings()["frame_count"])
        _drive_frame_beyond(tf, base_d, "(D) driven frame after clear")
        ft_d = tf.frame_timings()
        _assert_unpaced_shape(ft_d)

        # ── (E) jank_ratio re-derives from the integer counts ───────
        tf.set_fps(2)
        base_e = int(tf.frame_timings()["frame_count"])
        _drive_frame_beyond(tf, base_e, "(E) driven frame")
        ft_e = tf.frame_timings()
        assert ft_e["budget_us"] == 500_000, (
            f"2-fps target ⇒ 500_000µs budget; got {ft_e.get('budget_us')!r}"
        )
        over_e = int(ft_e["over_budget_frames"])
        len_e = int(ft_e["window_len"])
        rederived = over_e / len_e if len_e else 0.0
        assert abs(float(ft_e["jank_ratio"]) - rederived) <= 1e-6, (
            f"jank_ratio ({ft_e['jank_ratio']}) must re-derive from "
            f"{over_e}/{len_e}"
        )
        # 0.5s budget: still no healthy 8-row frame exceeds it.
        assert over_e == 0, f"0.5s budget ⇒ no jank; got {over_e}"

        # ── (F) Unknown-window rejection on write and read ──────────
        ghost_write_rejected = False
        try:
            tf.set_fps(60, window="ghost-window")
        except RpcError as exc:
            ghost_write_rejected = True
            assert exc.code == -32602, f"unknown-window write code -32602; got {exc.code}"
        assert ghost_write_rejected, "scene/set_fps on an unknown window must be rejected"

        ghost_read_rejected = False
        try:
            tf.frame_timings(window="ghost-window")
        except RpcError as exc:
            ghost_read_rejected = True
            assert exc.code == -32602, f"unknown-window read code -32602; got {exc.code}"
        assert ghost_read_rejected, "scene/frame_timings on an unknown window must be rejected"


if __name__ == "__main__":
    raise SystemExit(run_demo("r925_frame_budget", body))
