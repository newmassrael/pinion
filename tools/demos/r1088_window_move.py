#!/usr/bin/env python3
"""R1088 §5.16 §5.41 §2 #7 PR-31 — AI reposition write-peer
(`scene/window_move`) + declared-position convergence.

Drives `hello-dock-panels` (the `windows_signal` consumer) to exercise the
WRITE peer of the R1087 `scene/windows` read: `scene/window_move` lets an AI
agent reposition a declared floating window symbolically — the closure
writes `WindowSpec::position` into the binding's signal, the reconcile move
pass then drives the live OS window. Together `scene/windows` (read) +
`scene/window_move` (write) complete the introspect + intervene pair for the
floating-panel-as-positioned-window model.

HONEST SCOPE (headless): this verifies the write-peer + the declared
position SSOT end-to-end through the binding -> `windows_signal` ->
`reconcile_windows` arc. The LIVE OS placement, the user-`Moved` feedback
(declared<-actual convergence), and drag-to-follow are the HW-gated slice
(a real `WindowEvent::Moved` cannot be synthesized headlessly); their pure
decision core is unit-tested (`r1088_moved_echo_tests`,
`window_move::tests`).

Section roadmap (>=30 assertions across A-G):

  (A) Setup — tear off the inspector into a floating positioned window at
      the binding's cascade offset (the R1087 foundation; the move target).
  (B) Reposition writes the declared position — `scene/window_move` moves
      the floating window; `scene/windows` reflects the new position and the
      outcome envelope echoes it. A second move confirms it is repeatable.
  (C) UnknownWindow — a ghost id surfaces the precise `UnknownWindow`
      failure, not a silent no-op.
  (D) Idempotent re-move — moving to the CURRENT position is an accepted
      committing no-op (success, position unchanged).
  (E) Explicit AI move PINS a WM-placed window — `main` boots null
      (WM-placed); a `scene/window_move` on it sets `Some` (the explicit
      reposition semantics, distinct from the conservative user-`Moved`
      feedback which leaves a null window WM-managed).
  (F) Per-window isolation — moving one floating window leaves every other
      declared window's position untouched.
  (G) Read==write symmetry — every position read back equals the last one
      written, exactly (the SSOT round-trip).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

# ─── constants mirrored from the binding ────────────────────────────

_INSPECTOR_PANEL_TAG = "inspector"
_PROPERTY_PANEL_TAG = "property"

_FLOATING_PREFIX = "torn-"


def _floating_id(panel_id: str) -> str:
    return f"{_FLOATING_PREFIX}{panel_id}"


# `floating_window_position` in the binding: base (120, 90) + step*40, with
# step = inspector 0 / property 1 / viewport 2.
def _expected_position(panel_id: str) -> tuple[int, int]:
    step = {_INSPECTOR_PANEL_TAG: 0, _PROPERTY_PANEL_TAG: 1, "viewport": 2}[panel_id]
    return (120 + step * 40, 90 + step * 40)


# ─── scene/windows + scene/window_move helpers ──────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None, "scene/windows returned no response"
    result = resp.result
    assert isinstance(result, dict), f"scene/windows result must be an object; got {result!r}"
    windows = result.get("windows")
    assert isinstance(windows, list), f"scene/windows.windows must be a list; got {windows!r}"
    return windows


def _window_by_id(tf: RpcSubprocess, window_id: str) -> Optional[dict]:
    for w in _windows(tf):
        if w.get("id") == window_id:
            return w
    return None


def _position_of(tf: RpcSubprocess, window_id: str) -> Any:
    w = _window_by_id(tf, window_id)
    assert w is not None, f"scene/windows must list {window_id!r}"
    return w.get("position")


def _window_move(tf: RpcSubprocess, window_id: str, x: int, y: int) -> dict:
    """`scene/window_move` — the WRITE peer. Returns the outcome envelope.

    The target is `window_id`, NOT `window` (the reserved per-dispatch
    scope key); naming it `window` would route the request through the
    unknown-window scope gate instead of this method."""
    resp = tf.request("scene/window_move", {"window_id": window_id, "x": x, "y": y})
    assert resp is not None, "scene/window_move returned no response"
    result = resp.result
    assert isinstance(result, dict), f"window_move result must be an object; got {result!r}"
    return result


def _toggle_float(tf: RpcSubprocess, panel_id: str) -> None:
    tf.invoke(f"/{panel_id}/external/tear_off", None)


def _wait_listed(tf: RpcSubprocess, window_id: str) -> dict:
    return wait_until(
        lambda: _window_by_id(tf, window_id),
        desc=f"scene/windows lists {window_id}",
    )


def _wait_position(tf: RpcSubprocess, window_id: str, expected: list[int]) -> None:
    """The reposition write is synchronous in dispatch, but poll for
    zero-flake robustness against any reconcile ordering."""
    wait_until(
        lambda: _position_of(tf, window_id) == expected,
        desc=f"{window_id} declared position == {expected}",
    )


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) setup: a floating positioned window to move ───────
        boot = _windows(tf)
        assert len(boot) == 1 and boot[0].get("id") == "main", (
            f"boot must be the single 'main' window; got {boot!r}"
        )
        assert boot[0].get("position") is None, "main boots WM-placed (null)"

        _toggle_float(tf, _INSPECTOR_PANEL_TAG)
        torn_inspector = _floating_id(_INSPECTOR_PANEL_TAG)
        entry = _wait_listed(tf, torn_inspector)
        start = list(_expected_position(_INSPECTOR_PANEL_TAG))
        assert entry.get("position") == start, (
            f"{torn_inspector} must start at the cascade offset {start}; got {entry!r}"
        )

        # ── (B) reposition writes the declared position ───────────
        target = [start[0] + 200, start[1] + 130]
        outcome = _window_move(tf, torn_inspector, target[0], target[1])
        # The outcome envelope echoes the requested placement (the
        # confirmation anchor, mirroring ResizeOutcome).
        assert outcome.get("window_id") == torn_inspector, (
            f"outcome must echo the moved window id; got {outcome!r}"
        )
        assert outcome.get("x") == target[0] and outcome.get("y") == target[1], (
            f"outcome must echo the requested (x, y); got {outcome!r}"
        )
        assert outcome.get("requested") is True, f"a successful move is requested=true; got {outcome!r}"
        _wait_position(tf, torn_inspector, target)
        assert _position_of(tf, torn_inspector) == target, (
            f"scene/windows must reflect the moved position {target}; got "
            f"{_position_of(tf, torn_inspector)!r}"
        )
        # Repeatable: a second move to a fresh spot also lands.
        target2 = [target[0] - 60, target[1] + 45]
        _window_move(tf, torn_inspector, target2[0], target2[1])
        _wait_position(tf, torn_inspector, target2)
        assert _position_of(tf, torn_inspector) == target2, "second reposition must also land"
        # The move touched only the floating window — main stays WM-placed.
        assert _position_of(tf, "main") is None, "moving a floating window must not touch main"

        # ── (C) UnknownWindow on a ghost id ───────────────────────
        ghost_raised = False
        try:
            _window_move(tf, "torn-ghost-does-not-exist", 10, 10)
        except RpcError as e:
            ghost_raised = True
            blob = f"{e.message} {e.data!r}"
            assert "UnknownWindow" in blob, (
                f"a ghost window id must surface UnknownWindow; got {blob!r}"
            )
        assert ghost_raised, "scene/window_move on a ghost id must raise, not silently no-op"
        # The ghost move changed nothing.
        assert _position_of(tf, torn_inspector) == target2, "a failed move must not perturb state"

        # ── (D) idempotent re-move (current position) ─────────────
        same = _window_move(tf, torn_inspector, target2[0], target2[1])
        assert same.get("requested") is True, "re-moving to the current position still succeeds"
        assert _position_of(tf, torn_inspector) == target2, (
            "an idempotent re-move leaves the position unchanged"
        )

        # ── (E) explicit AI move PINS a WM-placed window ──────────
        assert _position_of(tf, "main") is None, "precondition: main is still WM-placed (null)"
        main_target = [300, 220]
        main_outcome = _window_move(tf, "main", main_target[0], main_target[1])
        assert main_outcome.get("requested") is True, "pinning main is a successful move"
        _wait_position(tf, "main", main_target)
        assert _position_of(tf, "main") == main_target, (
            f"an explicit move must PIN a WM-placed window (None->Some); got "
            f"{_position_of(tf, 'main')!r}"
        )

        # ── (F) per-window isolation ──────────────────────────────
        _toggle_float(tf, _PROPERTY_PANEL_TAG)
        torn_property = _floating_id(_PROPERTY_PANEL_TAG)
        _wait_listed(tf, torn_property)
        prop_pos_before = _position_of(tf, torn_property)
        # Move the inspector again; the property window must not budge.
        iso_target = [500, 400]
        _window_move(tf, torn_inspector, iso_target[0], iso_target[1])
        _wait_position(tf, torn_inspector, iso_target)
        assert _position_of(tf, torn_property) == prop_pos_before, (
            f"moving one window must leave others untouched; {torn_property} drifted from "
            f"{prop_pos_before!r} to {_position_of(tf, torn_property)!r}"
        )
        assert _position_of(tf, "main") == main_target, "main stays where it was pinned"

        # ── (G) read==write symmetry (SSOT round-trip) ────────────
        for wx, wy in ((111, 222), (333, 44), (7, 9)):
            _window_move(tf, torn_inspector, wx, wy)
            _wait_position(tf, torn_inspector, [wx, wy])
            got = _position_of(tf, torn_inspector)
            assert got == [wx, wy], f"read must equal the written position [{wx}, {wy}]; got {got!r}"
            assert isinstance(got, list) and len(got) == 2 and all(isinstance(c, int) for c in got), (
                f"a read-back position must be a [x, y] integer pair; got {got!r}"
            )


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1088 §5.16 §5.41 §2 #7 PR-31 — scene/window_move AI reposition write-peer",
        body,
    ))
