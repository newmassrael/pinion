#!/usr/bin/env python3
"""R1570.3 — the demo harness's process lifecycle, tested.

`tools/rpc_verify.py` drives every demo in this repo and, until R1570.3, nothing
verified any of it. That is the same gap R1495 closed for the hook libraries,
and it cost the same way: `shutdown` did `SIGTERM; wait(2); kill(); wait(1)` and
let the second `wait`'s `TimeoutExpired` escape **while leaving the process
running**. In CI the four binaries R1570 had left spinning became four orphans,
and the 33 demos that ran after them failed in a set that differed between runs
— four deterministic failures reported as thirty-seven non-deterministic ones.

What is tested here is `terminate_process_tree`, the one function teardown goes
through. The subjects are synthesised rather than borrowed from the tree,
because the interesting cases are processes a demo binary is not: one that
ignores `SIGTERM`, and one that leaves a child behind.

Run from the workspace root (fast — no cargo, no display):
    python3 tools/test_rpc_verify.py
"""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from rpc_verify import terminate_process_tree  # noqa: E402

PASSED = 0
FAILED: list[str] = []


def check(cond: bool, label: str) -> None:
    global PASSED
    if cond:
        PASSED += 1
    else:
        FAILED.append(label)
        print(f"  FAIL {label}")


def spawn(script: str) -> subprocess.Popen:
    """Launch a python one-liner the way `RpcSubprocess` launches a binary."""
    return subprocess.Popen(
        [sys.executable, "-c", script],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )


def alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def wait_gone(pid: int, budget: float = 5.0) -> bool:
    deadline = time.time() + budget
    while time.time() < deadline:
        if not alive(pid):
            return True
        time.sleep(0.02)
    return False


# ── a cooperative process is reaped by SIGTERM ──────────────────────────────
def test_cooperative() -> None:
    proc = spawn("import time; time.sleep(300)")
    leak = terminate_process_tree(proc)
    check(leak is None, "cooperative: reaped, no leak reported")
    check(proc.poll() is not None, "cooperative: the process really exited")


# ── a process that IGNORES SIGTERM is reaped by SIGKILL ─────────────────────
def test_ignores_sigterm() -> None:
    # The case the old body's `kill()` did handle — kept as the control, so a
    # regression that broke the escalation would not hide behind the group
    # change below.
    proc = spawn(
        "import signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); "
        "print('ready', flush=True); time.sleep(300)"
    )
    assert proc.stdout is not None
    proc.stdout.readline()
    leak = terminate_process_tree(proc, term_grace=0.3)
    check(leak is None, "sigterm-ignoring: escalated to SIGKILL and reaped")
    check(proc.poll() is not None, "sigterm-ignoring: the process really exited")


# ── a GRANDCHILD is reaped too — what `Popen.kill` cannot do ────────────────
def test_grandchild() -> None:
    # THE regression case. A demo binary that spawns a helper leaked the helper
    # under the old teardown, because `Popen.kill` signals the direct child
    # only. Signalling the process group is the whole fix, and this is the only
    # test that can tell the two apart.
    proc = spawn(
        "import subprocess, sys, time; "
        "k = subprocess.Popen([sys.executable, '-c', "
        "'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)']); "
        "print(k.pid, flush=True); time.sleep(300)"
    )
    assert proc.stdout is not None
    grandchild = int(proc.stdout.readline().strip())
    check(alive(grandchild), "grandchild: the helper is running before teardown")

    leak = terminate_process_tree(proc, term_grace=0.3)
    check(leak is None, "grandchild: the parent is reaped")
    check(
        wait_gone(grandchild),
        "grandchild: the HELPER is reaped too — the group signal is what does "
        "this, and `Popen.kill` would leave it running",
    )


# ── an already-dead process is not a leak ──────────────────────────────────
def test_already_exited() -> None:
    proc = spawn("pass")
    proc.wait(timeout=5)
    leak = terminate_process_tree(proc)
    check(leak is None, "already-exited: reported as no leak")


# ── a survivor is NAMED, not raised over ───────────────────────────────────
def test_survivor_is_reported() -> None:
    # A process this test cannot kill is not synthesisable portably (SIGKILL is
    # uncatchable), so the reporting PATH is exercised directly against a stub
    # whose waits always time out. Without this, the only untested branch would
    # be the one that exists for the exact situation the round is about.
    class Unkillable:
        pid = -1

        def poll(self) -> None:
            return None

        def wait(self, timeout: float | None = None) -> int:
            raise subprocess.TimeoutExpired(cmd="stub", timeout=timeout or 0.0)

        def send_signal(self, sig: int) -> None:
            pass

    leak = terminate_process_tree(Unkillable(), term_grace=0.01, kill_grace=0.01)  # type: ignore[arg-type]
    check(leak is not None, "survivor: a leak is REPORTED rather than swallowed")
    check(
        leak is not None and "SIGKILL" in leak and "-1" in leak,
        "survivor: the report names the pid and what was tried",
    )


# ── the report is a return value, so teardown can choose ───────────────────
def test_reporting_does_not_raise() -> None:
    # The defect was an exception escaping teardown. Whatever else changes, the
    # function must not be able to do that: a raise here would replace the
    # demo's own verdict with a teardown detail.
    class Unkillable:
        pid = -2

        def poll(self) -> None:
            return None

        def wait(self, timeout: float | None = None) -> int:
            raise subprocess.TimeoutExpired(cmd="stub", timeout=timeout or 0.0)

        def send_signal(self, sig: int) -> None:
            raise ProcessLookupError("gone")

    try:
        terminate_process_tree(Unkillable(), term_grace=0.01, kill_grace=0.01)  # type: ignore[arg-type]
        check(True, "reporting: survives a signal that itself fails")
    except Exception as exc:  # noqa: BLE001 — the point is that nothing escapes
        check(False, f"reporting: teardown raised {exc!r}")


def main() -> int:
    for fn in (
        test_cooperative,
        test_ignores_sigterm,
        test_grandchild,
        test_already_exited,
        test_survivor_is_reported,
        test_reporting_does_not_raise,
    ):
        fn()
    print(f"[harness] {PASSED} passed, {len(FAILED)} failed")
    if FAILED:
        for label in FAILED:
            print(f"  - {label}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
