#!/usr/bin/env python3
"""R810 §5.38 §5.12 — snackbar shown-state + live countdown as a first-class RPC query.

Before R810 the snackbar's visibility lived only in the `SnackbarTimer::visible`
Signal the view-fn reads to paint the overlay; an AI agent driving over the
§5.12 RPC plane had to *infer* "is the snackbar up?" from scene-tree presence
(the exact asymmetry R795 closed for modals). R810 adds a query-only
`SnackbarIntrospect` extra-external at `snackbar_state`, surfacing the live
countdown at `/snackbar_state/external/{visible,remaining,duration}` — a node in
the *state* scene, present shown OR hidden, so it answers either way. No second
source of truth: the flag + countdown still live once in `SnackbarTimer`; this
node only reads them.

The snackbar's countdown is a *live* value: under a continuously-rendering
backend the §5.28 animation driver advances it every frame (and the R724
`scene/tick` RPC can inject extra time on top — the deterministic driver for a
non-rendering/headless context). So this demo asserts the *observable* shape of
the live countdown rather than frozen values: it is queryable as data,
monotonically decreasing while shown, read-only over the wire, and round-trips
through show / auto-dismiss / re-show / explicit-dismiss.

  (A) boot → hidden, but visible/remaining/duration are queryable as data.
  (B) read-only → intervene on every slot (and an unknown slot) is refused.
  (C) show via the trigger → visible True, remaining within the horizon.
  (D) the live countdown is RPC-observable — remaining decreases over time.
  (E) scene/tick past the horizon → auto-dismiss (visible False, remaining 0).
  (F) re-show restarts the countdown.
  (G) the UNDO action dismisses immediately, observable as data.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

VIEWPORT = (360, 300)
VIS = "/snackbar_state/external/visible"
REM = "/snackbar_state/external/remaining"
DUR = "/snackbar_state/external/duration"
HORIZON = 4.0  # SNACK_DURATION_SECS in hello-snackbar


def _present(node, tag: str) -> bool:
    if isinstance(node, dict):
        if node.get("tag") == tag:
            return True
        return any(_present(ch, tag) for ch in node.get("children") or [])
    return False


def body() -> None:
    with RpcSubprocess("hello-snackbar", boot_grace=1.5) as d:

        def rnum(path: str):
            v = d.query(path)
            return float(v) if isinstance(v, (int, float)) and not isinstance(v, bool) else None

        def wait_vis(expected: bool) -> None:
            wait_until(lambda: d.query(VIS) == expected, desc=f"visible == {expected}")

        # ── (A) boot: hidden, but the countdown is queryable as DATA ──
        assert_eq(d.query(VIS), False, "boot: snackbar introspects as hidden (no paint walk)")
        assert rnum(REM) == 0.0, f"boot remaining is exactly 0 while hidden; got {d.query(REM)!r}"
        assert abs(rnum(DUR) - HORIZON) < 0.01, f"boot duration = M3 default 4s; got {d.query(DUR)!r}"

        # ── (B) every slot is read-only (the countdown is timer-driven) ──
        for slot, label in ((VIS, "visible"), (REM, "remaining"), (DUR, "duration")):
            refused = False
            try:
                d.intervene(slot, True)
            except RpcError:
                refused = True
            assert refused, f"{label}: a raw write must be refused (ReadOnly) — it would desync the countdown"
        assert_eq(d.query(VIS), False, "the refused writes left the flag untouched")
        assert rnum(REM) == 0.0, "the refused writes left remaining untouched"
        refused = False
        try:
            d.intervene("/snackbar_state/external/elapsed", True)
        except RpcError:
            refused = True
        assert refused, "an unknown slot intervene is refused (UnknownPath)"

        # ── (C) show via the trigger button ──────────────────────────
        d.click(path="show_snack")
        wait_vis(True)
        assert_eq(d.query(VIS), True, "show: visible flips True over RPC (the asymmetry R810 removes)")
        r0 = rnum(REM)
        assert r0 is not None and 0.0 < r0 <= HORIZON + 0.05, f"show: remaining within the horizon; got {r0!r}"
        assert abs(rnum(DUR) - HORIZON) < 0.01, "show: duration is the 4s horizon"
        assert _present(d.snapshot(source="paint", viewport=VIEWPORT), "snackbar"), (
            "the overlay paints while shown (scene-as-data still works; the AI no longer must walk for it)"
        )

        # ── (D) the live countdown is RPC-observable (monotone down) ──
        prev = rnum(REM)
        for step in range(3):
            # Observed-state polling on the wall-clock countdown (R883
            # zero-flake): wait for the live value to drop below the
            # previous sample instead of betting on a fixed sleep.
            cur = wait_until(
                lambda: (lambda v: v if v is not None and v < prev else None)(rnum(REM)),
                desc=f"step {step}: live countdown decreases over time",
            )
            assert_eq(d.query(VIS), True, f"step {step}: still shown mid-countdown")
            prev = cur

        # ── (E) scene/tick past the horizon → auto-dismiss as data ───
        d.tick(HORIZON + 1.0)  # inject more than any remaining time → cross the horizon
        wait_vis(False)
        assert_eq(d.query(VIS), False, "auto-dismissed once the countdown is exhausted")
        assert rnum(REM) == 0.0, "remaining is exactly 0 while hidden"
        assert not _present(d.snapshot(source="paint", viewport=VIEWPORT), "snackbar"), (
            "the overlay is gone after dismissal"
        )

        # ── (F) re-show restarts the countdown ───────────────────────
        d.click(path="show_snack")
        wait_vis(True)
        assert_eq(d.query(VIS), True, "re-shown")
        r1 = rnum(REM)
        assert r1 is not None and 0.0 < r1 <= HORIZON + 0.05, "re-show restarts the countdown within the horizon"
        assert _present(d.snapshot(source="paint", viewport=VIEWPORT), "snackbar"), "overlay paints again"

        # ── (G) the UNDO action dismisses immediately, as data ───────
        d.click(path="snack_undo")
        wait_vis(False)
        assert_eq(d.query(VIS), False, "UNDO dismisses immediately (observable over RPC)")
        assert rnum(REM) == 0.0, "remaining is 0 after the explicit dismiss"

        # ── (H) unknown query slot → error (only the 3 slots exist) ──
        refused = False
        try:
            d.query("/snackbar_state/external/elapsed")
        except RpcError:
            refused = True
        assert refused, "an unknown query slot is rejected (only visible/remaining/duration exist)"


if __name__ == "__main__":
    sys.exit(run_demo("R810 §5.38 §5.12 — snackbar shown-state RPC query", body))
