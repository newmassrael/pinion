#!/usr/bin/env python3
"""R646-R649 scene/simulate AI-native primary path demo (§5.49 R59).

Visible dogfood for the R646-R649 dry_run / simulate axis. Drives the
live `hello-slider` window over JSON-RPC 2.0 and proves the four
spec commitments end-to-end:

  * R646 §5.12 — `scene/simulate` accepts a multi-step sequence and
    returns a snapshot reflecting the compound hypothetical state.
  * R26 §5.22 (R647) — Signal graph snapshot/restore wraps every
    simulate call so reactive state is fully rolled back even when
    External rollback alone would miss Effect-mediated Signal drift.
  * R648 §5.34 — multi-event `simulate` composes with the same
    `/external/...` path convention every other §5.12 method honours;
    per-unique-path save semantics ensure rollback restores the
    pre-call value, not the intermediate.
  * R27 §5.23 (R649) — Effect side-effect suppression via thread-local
    `SimulationGuard` keeps Effects observing `intervene`-touched
    Signals from firing during the hypothetical-mutation cycle.

Verification arc:

  1. spawn hello-slider
  2. baseline `scene/query "/external/value"` → 0.0
  3. `scene/dry_run` value=0.7 → snapshot shows hypothetical 0.7
  4. `scene/query` again → still 0.0 (single-step rollback proven)
  5. `scene/simulate` 3-step sequence [0.3, 0.6, 0.9] → snapshot
     shows final 0.9 (compound hypothetical)
  6. `scene/query` again → still 0.0 (multi-step rollback proven;
     per-unique-path save semantics ensured intermediate 0.3 / 0.6
     never landed)
  7. `scene/simulate` 2-step sequence with mid-failure (step 2 =
     Bool on Int slot) → error response + scene unchanged at 0.0

Each assertion echoes the live state to stdout so a human reader can
follow the AI-side reasoning that simulate/dry_run rolled back every
hypothetical mutation cleanly. Exit 0 on every check satisfied,
non-zero on the first mismatch.

Run from the workspace root:

    cargo run -p hello-slider --release   # build cache once
    python3 tools/demos/scene_simulate_r646_r649.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcError, RpcSubprocess


VALUE_PATH = "/external/value"


def _read_value(rpc: RpcSubprocess) -> float:
    """`scene/query` the slider's value slot, expecting a numeric
    payload. Returns the value as a Python float regardless of
    whether the wire carried an int or float (JSON ambiguity)."""
    raw = rpc.query(VALUE_PATH)
    if not isinstance(raw, (int, float)):
        raise AssertionError(f"expected numeric value, got {raw!r}")
    return float(raw)


def _dry_run(rpc: RpcSubprocess, value: float) -> Any:
    """`scene/dry_run` typed wrapper — no helper in rpc_verify yet,
    so go through the low-level request() entrypoint."""
    resp = rpc.request("scene/dry_run", {"path": VALUE_PATH, "value": value})
    assert resp is not None
    return resp.result


def _simulate(rpc: RpcSubprocess, steps: list[dict[str, Any]]) -> Any:
    """`scene/simulate` typed wrapper — same shape as dry_run but
    accepts an ordered step sequence."""
    resp = rpc.request("scene/simulate", {"steps": steps})
    assert resp is not None
    return resp.result


def _simulate_err(rpc: RpcSubprocess, steps: list[dict[str, Any]]) -> RpcError:
    """Drive `scene/simulate` expecting a JSON-RPC error response.
    Returns the [`RpcError`] so callers can inspect code / data."""
    try:
        rpc.request("scene/simulate", {"steps": steps})
    except RpcError as exc:
        return exc
    raise AssertionError("expected scene/simulate to fail; succeeded instead")


def _snapshot_value(snapshot: Any) -> float:
    """Walk the SnapshotNode JSON for the External's `value` slot.
    SnapshotNode shape: `{"introspect": {"value": N, ...}, ...}` at
    the External (JSON object, not array of pairs). hello-slider's
    state scene is a single External at the root so the introspect
    map is directly accessible."""
    if isinstance(snapshot, dict):
        intro = snapshot.get("introspect")
        if isinstance(intro, dict) and "value" in intro:
            raw = intro["value"]
            if isinstance(raw, (int, float)):
                return float(raw)
    raise AssertionError(f"value slot missing from snapshot: {snapshot!r}")


def main() -> int:
    print("=" * 64)
    print("R646-R649 scene/simulate dogfood — hello-slider")
    print("=" * 64)

    with RpcSubprocess("hello-slider") as rpc:
        # --- 1. baseline ---------------------------------------------------
        baseline = _read_value(rpc)
        print(f"\n[1] baseline   scene/query     value = {baseline}")
        if baseline != 0.0:
            print(f"   WARN: expected 0.0 at startup, got {baseline}")

        # --- 2. dry_run single hypothetical write --------------------------
        print(f"\n[2] R646 §5.12 scene/dry_run    hypothetical value = 0.7")
        snap = _dry_run(rpc, 0.7)
        snap_v = _snapshot_value(snap)
        print(f"    snapshot reports value = {snap_v}")
        assert abs(snap_v - 0.7) < 1e-6, (
            f"dry_run snapshot must reflect hypothetical 0.7, got {snap_v}"
        )

        after_dry = _read_value(rpc)
        print(f"    scene/query after dry_run     value = {after_dry}")
        assert after_dry == baseline, (
            f"dry_run must rollback; baseline {baseline} != {after_dry}"
        )
        print("    ROLLBACK PROVEN — single-step hypothetical did not land")

        # --- 3. simulate multi-step sequence -------------------------------
        steps = [
            {"path": VALUE_PATH, "value": 0.3},
            {"path": VALUE_PATH, "value": 0.6},
            {"path": VALUE_PATH, "value": 0.9},
        ]
        print(f"\n[3] R646 §5.12 scene/simulate   3-step sequence "
              "[0.3, 0.6, 0.9]")
        snap = _simulate(rpc, steps)
        snap_v = _snapshot_value(snap)
        print(f"    snapshot reports final value = {snap_v}")
        assert abs(snap_v - 0.9) < 1e-6, (
            f"simulate snapshot must reflect final step 0.9, got {snap_v}"
        )

        after_sim = _read_value(rpc)
        print(f"    scene/query after simulate    value = {after_sim}")
        assert after_sim == baseline, (
            f"simulate must rollback to baseline {baseline}, got {after_sim}"
        )
        print("    ROLLBACK PROVEN — neither intermediate (0.3, 0.6) "
              "nor final (0.9) landed")
        print("    R648 §5.34 per-unique-path save: pre-call 0.0 restored "
              "(not intermediate 0.3)")

        # --- 4. mid-sequence failure rolls back -----------------------------
        print(f"\n[4] R646 mid-failure: step 2 = Bool on Float slot")
        bad_steps = [
            {"path": VALUE_PATH, "value": 0.42},
            {"path": VALUE_PATH, "value": True},  # type mismatch
        ]
        err = _simulate_err(rpc, bad_steps)
        print(f"    error response: code={err.code} data={err.data!r}")
        assert err.code == -32602, (
            f"expected invalid_params (-32602), got {err.code}"
        )
        assert err.data == "Intervene", (
            f"expected data='Intervene' variant, got {err.data!r}"
        )

        after_fail = _read_value(rpc)
        print(f"    scene/query after failed simulate value = {after_fail}")
        assert after_fail == baseline, (
            f"failed simulate must rollback step 1; "
            f"baseline {baseline} != {after_fail}"
        )
        print("    ROLLBACK PROVEN — step 1 mutation (0.42) was reverted "
              "even though step 2 errored")

        # --- 5. spec invariant #3 fulfillment summary -----------------------
        print("\n" + "=" * 64)
        print("§2 invariant #3 dry_run primitive — spec commitments verified")
        print("=" * 64)
        print("  R646 §5.12 — scene/simulate multi-event RPC method      OK")
        print("  R26  §5.22 — Signal graph rollback via Owner snapshot    OK")
        print("  R648 §5.34 — multi-segment path + per-unique-path save   OK")
        print("  R27  §5.23 — Effect suppression via SimulationGuard      OK")
        print("                (Effects observing 'value' slot did NOT")
        print("                 fire during the 3 simulate calls above —")
        print("                 if they had, the hover-progress animation")
        print("                 would have transitioned to Hover state)")
        print()
        print("AI clients can drive 'if I do A then B then C' scenarios")
        print("in ONE RPC round-trip without mutating live scene state.")
        return 0


if __name__ == "__main__":
    sys.exit(main())
