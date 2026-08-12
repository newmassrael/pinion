#!/usr/bin/env python3
"""R1570.3 / R1580 — the demo harness, tested.

`tools/rpc_verify.py` drives every demo in this repo and, until R1570.3, nothing
verified any of it. That is the same gap R1495 closed for the hook libraries,
and it cost the same way: `shutdown` did `SIGTERM; wait(2); kill(); wait(1)` and
let the second `wait`'s `TimeoutExpired` escape **while leaving the process
running**. In CI the four binaries R1570 had left spinning became four orphans,
and the 33 demos that ran after them failed in a set that differed between runs
— four deterministic failures reported as thirty-seven non-deterministic ones.

R1570.3 tested `terminate_process_tree`, the one function teardown goes through.
The subjects are synthesised rather than borrowed from the tree, because the
interesting cases are processes a demo binary is not: one that ignores
`SIGTERM`, and one that leaves a child behind.

R1580 covers the rest, which is what every demo calls on every line: the wire
(`request` — id matching and the notification split R1552 warned about), the
clock (`wait_until`, where the zero-flake policy is actually enforced), the
readers (`find_by_tag`, `access_node_by_tag`, `assert_eq`), the injectors
(`key`'s argument rules) and the boot gate (`PINION_ASSUME_BUILT`). None of it
needs a process: `request` runs against a fake pipe, `wait_until` against a
deterministic predicate, the readers against literal scene dicts. The suite
still finishes in well under a second, which is what keeps it inside
`pre-push`.

Run from the workspace root (fast — no cargo, no display):
    python3 tools/test_rpc_verify.py
"""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rpc_verify  # noqa: E402
from rpc_verify import (  # noqa: E402
    Response,
    RpcError,
    RpcSubprocess,
    _pointer_reach_budget,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    terminate_process_tree,
    wait_until,
)

PASSED = 0
FAILED: list[str] = []


#: The case currently running, so every verdict names where it came from.
#: R1580 — without it a failure label like "and the last value it saw" does not
#: say which case produced it, and a case that RAISES is reported in a different
#: vocabulary from one that merely fails. One vocabulary, one prefix.
CURRENT = ""


def check(cond: bool, label: str) -> None:
    global PASSED
    if cond:
        PASSED += 1
    else:
        FAILED.append(f"{CURRENT}: {label}" if CURRENT else label)
        print(f"  FAIL {FAILED[-1]}")


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


# ---------------------------------------------------------------------------
# R1580 — the wire. `request` is what every demo line goes through, and its
# hardest rule is one R1552 wrote down as a hazard: a server-initiated frame
# must be KEPT, because discarding it loses somebody's subscription stream.
# ---------------------------------------------------------------------------


class FakePipe:
    """Stands in for the child's stdin: records what the harness wrote."""

    def __init__(self) -> None:
        self.written: list[str] = []

    def write(self, text: str) -> None:
        self.written.append(text)

    def flush(self) -> None:
        pass


class FakeProc:
    """A child that is alive unless told otherwise."""

    pid = -3

    def __init__(self, returncode: int | None = None) -> None:
        self.stdin = FakePipe()
        self.returncode = returncode

    def poll(self) -> int | None:
        return self.returncode


def wired(returncode: int | None = None) -> RpcSubprocess:
    """An `RpcSubprocess` attached to a fake pipe rather than a binary.

    Built through `__init__` so the object under test is the real one; only the
    two attributes that touch the OS are replaced. `ensure_build=False` because
    constructing one must not shell out to cargo.
    """
    tf = RpcSubprocess("nonexistent-example", ensure_build=False, request_timeout=0.4)
    tf._proc = FakeProc(returncode)  # type: ignore[assignment]
    return tf


def deliver(tf: RpcSubprocess, frame: dict) -> None:
    tf._inbox.put(json.dumps(frame))


def sent(tf: RpcSubprocess) -> list[dict]:
    return [json.loads(line) for line in tf._proc.stdin.written]  # type: ignore[union-attr]


def test_request_matches_its_own_id() -> None:
    tf = wired()
    deliver(tf, {"jsonrpc": "2.0", "id": 999, "result": "somebody else's"})
    deliver(tf, {"jsonrpc": "2.0", "id": 1, "result": "mine"})
    reply = tf.request("scene/probe")
    check(reply is not None and reply.result == "mine",
          "request: answers with the frame carrying its own id")
    check(sent(tf)[0]["method"] == "scene/probe", "request: sends the method")
    check("params" not in sent(tf)[0],
          "request: omits params entirely when none were given")


def test_request_keeps_notifications() -> None:
    # R1552's own warning, as an assertion: a frame with a `method` and no `id`
    # is a subscription stream, not noise. `tools/rpc_verify.py` resolving on
    # the first matching id and DISCARDING the rest is what the wire form was
    # chosen to survive.
    tf = wired()
    deliver(tf, {"jsonrpc": "2.0", "method": "scene/advanced", "params": {"n": 1}})
    deliver(tf, {"jsonrpc": "2.0", "id": 1, "result": "ok"})
    reply = tf.request("scene/probe")
    check(reply is not None and reply.result == "ok",
          "notifications: the answer still arrives past a notification")
    kept = tf.notifications()
    check(len(kept) == 1 and kept[0]["method"] == "scene/advanced",
          "notifications: the stream frame is KEPT, not dropped")
    check(tf.notifications("scene/advanced") == kept,
          "notifications: filtering by method finds it")
    check(tf.notifications("other") == [],
          "notifications: and does not find one that is not there")


def test_request_raises_the_surfaces_error() -> None:
    tf = wired()
    deliver(tf, {"jsonrpc": "2.0", "id": 1,
                 "error": {"code": -32005, "message": "refused", "data": "why"}})
    try:
        tf.request("scene/act")
        check(False, "error: an error frame must raise")
    except RpcError as exc:
        check(exc.code == -32005 and exc.message == "refused" and exc.data == "why",
              "error: the code, the message and the payload all survive")


def test_request_times_out_naming_the_call() -> None:
    tf = wired()
    try:
        tf.request("scene/never")
        check(False, "timeout: a silent surface must raise")
    except RpcError as exc:
        check("scene/never" in str(exc) and "id=1" in str(exc),
              "timeout: the refusal names the method and the id")


def test_request_on_a_dead_child_says_so() -> None:
    tf = wired(returncode=101)
    try:
        tf.request("scene/probe")
        check(False, "dead child: must raise rather than block")
    except RpcError as exc:
        check("101" in str(exc), "dead child: the exit code is reported")


def test_notify_sends_no_id_and_does_not_wait() -> None:
    tf = wired()
    reply = tf.request("scene/tick", {"seconds": 0.016}, notify=True)
    frame = sent(tf)[0]
    check(reply is None, "notify: answers nothing")
    check("id" not in frame, "notify: carries no id, so nothing can answer it")
    check(frame["params"] == {"seconds": 0.016}, "notify: params travel")


def test_drain_reads_without_sending() -> None:
    tf = wired()
    deliver(tf, {"jsonrpc": "2.0", "method": "scene/advanced"})
    drained = tf.drain_notifications()
    check(len(drained) == 1, "drain: collects a frame with no request in flight")
    check(sent(tf) == [], "drain: and sends nothing to do it")


# ---------------------------------------------------------------------------
# R1580 — the clock. `wait_until` is where [[zero-flake-policy]] is actually
# enforced: every demo that observes a post-action state goes through it.
# ---------------------------------------------------------------------------


def test_wait_until_returns_the_value() -> None:
    seen = []

    def predicate():
        seen.append(1)
        return "ready" if len(seen) >= 3 else None

    got = wait_until(predicate, timeout=2.0, interval=0.001, desc="ready")
    check(got == "ready", "wait_until: returns the truthy value, not just True")
    check(len(seen) == 3, "wait_until: stops polling once it is satisfied")


def test_wait_until_polls_once_even_with_no_budget() -> None:
    # The predicate is evaluated BEFORE the deadline is consulted, so an
    # already-true condition never fails on a zero budget. A loop written the
    # other way round makes every demo's first check load-dependent.
    check(wait_until(lambda: "now", timeout=0.0, interval=0.001) == "now",
          "wait_until: a condition already true needs no budget")


def test_wait_until_reports_what_it_last_saw() -> None:
    try:
        wait_until(lambda: 0, timeout=0.01, interval=0.001, desc="the panel opens")
        check(False, "wait_until: must raise when the budget runs out")
    except AssertionError as exc:
        text = str(exc)
        check("the panel opens" in text, "wait_until: the message carries the desc")
        check("last=0" in text, "wait_until: and the last value it saw")
        check("0.01s" in text, "wait_until: and the budget it spent")


def test_wait_until_honours_its_interval() -> None:
    calls = []
    try:
        wait_until(lambda: calls.append(1), timeout=0.12, interval=0.04)
    except AssertionError:
        pass
    # 0.12s at 0.04s apart is ~4 polls. A loop ignoring `interval` spins
    # thousands of times and would burn a CI core per waiting demo.
    check(1 <= len(calls) <= 12,
          f"wait_until: polls at the stated interval (saw {len(calls)})")


# ---------------------------------------------------------------------------
# R1580 — the readers, against literal scenes. `find_by_tag` DESCENDS a
# container and a scroll and must NOT descend a Text's `content`, which is a
# string; an access tree is FLAT and is scanned instead.
# ---------------------------------------------------------------------------


def test_find_by_tag_descends_the_right_things() -> None:
    scene = {
        "type": "Container", "tag": "root",
        "children": [
            {"type": "Text", "tag": "label", "content": "not a child"},
            {"type": "Scroll", "tag": "scroll",
             "content": {"type": "Container", "tag": "deep", "children": []}},
        ],
    }
    check(find_by_tag(scene, "root") is scene, "find_by_tag: finds the root itself")
    check((find_by_tag(scene, "label") or {}).get("content") == "not a child",
          "find_by_tag: descends Container.children")
    check(find_by_tag(scene, "deep") is not None,
          "find_by_tag: descends Scroll.content")
    check(find_by_tag(scene, "absent") is None,
          "find_by_tag: answers None rather than raising")
    check(find_by_tag("not a node", "root") is None,
          "find_by_tag: a non-dict is not a scene")


def test_access_node_by_tag_scans_a_flat_list() -> None:
    tree = {"nodes": [{"tag": "a", "role": "button"}, {"tag": "b"}]}
    check((access_node_by_tag(tree, "a") or {}).get("role") == "button",
          "access_node_by_tag: finds a node in the flat list")
    check(access_node_by_tag(tree, "c") is None,
          "access_node_by_tag: absent is None")
    check(access_node_by_tag({}, "a") is None,
          "access_node_by_tag: an empty reply has no nodes")


def test_assert_eq_names_both_sides() -> None:
    assert_eq(1, 1, "equal values pass")
    check(True, "assert_eq: equal values do not raise")
    try:
        assert_eq("got", "want", "the readout")
        check(False, "assert_eq: unequal values must raise")
    except AssertionError as exc:
        text = str(exc)
        check("the readout" in text and "'want'" in text and "'got'" in text,
              "assert_eq: the message carries the label and both values")


# ---------------------------------------------------------------------------
# R1580 — the injectors and the boot gate.
# ---------------------------------------------------------------------------


def test_key_argument_rules() -> None:
    tf = wired()
    for label, call in (
        ("an empty name", lambda: tf.key(at=(1.0, 1.0), name="")),
        ("both a point and a path", lambda: tf.key(at=(1.0, 1.0), path="t", name="a")),
        ("neither a point nor a path", lambda: tf.key(name="a")),
    ):
        try:
            call()
            check(False, f"key: {label} must be refused")
        except ValueError:
            check(True, f"key: {label} is refused")

    # R882.1 — a real key RELEASE carries no cursor, so it needs no position.
    deliver(tf, {"jsonrpc": "2.0", "id": 1, "result": None})
    tf.key(name="Shift", state="up")
    frame = sent(tf)[-1]
    check(frame["params"] == {"key": "Shift", "state": "up"},
          "key: a release is positionless and says so")


def test_assume_built_gate_reads_the_value_not_the_presence() -> None:
    # R1333 — presence-checking made `PINION_ASSUME_BUILT=0` DISABLE the
    # rebuild, the opposite of what setting `0` means.
    for value, expect_build in (("1", False), ("0", True), (None, True)):
        previous = os.environ.get("PINION_ASSUME_BUILT")
        if value is None:
            os.environ.pop("PINION_ASSUME_BUILT", None)
        else:
            os.environ["PINION_ASSUME_BUILT"] = value
        try:
            tf = RpcSubprocess("nonexistent-example")
            check(tf.ensure_build is expect_build,
                  f"boot gate: PINION_ASSUME_BUILT={value!r} -> build={expect_build}")
        finally:
            if previous is None:
                os.environ.pop("PINION_ASSUME_BUILT", None)
            else:
                os.environ["PINION_ASSUME_BUILT"] = previous


def test_pointer_reach_gate_separates_the_two_unreachable_kinds() -> None:
    # R1650 — a covered widget FAILS (the repair is a declaration) and an
    # off-window one does not (the repair is a scroll region, a layout
    # decision). Getting these the same way round is what would make the gate
    # either useless or unadoptable.
    tf = RpcSubprocess("fixture-example")
    tf._proc = None
    report = {"deliverable": 1, "inert": 0, "shadows": [], "unreachable": []}
    tf.request = lambda method, params=None, **kw: Response(  # type: ignore[assignment]
        id=1, result=report
    )
    tf._gate_pointer_reach()  # clean surface: no raise

    report["unreachable"] = [{"tag": "row", "path": "row", "blocked_by": None}]
    tf._gate_pointer_reach()  # off-window: reported, not fatal

    report["unreachable"] = [{"tag": "board", "path": "", "blocked_by": "card"}]
    raised = None
    try:
        tf._gate_pointer_reach()
    except AssertionError as exc:
        raised = str(exc)
    check(raised is not None, "gate: a covered widget fails the demo")
    check(
        raised is not None and "board" in raised and "card" in raised,
        "gate: the failure names BOTH the widget and what covers it",
    )

    # And the budget is what makes it adoptable — the same row, allowed.
    tf.pointer_reach_exempt = {"board": "a fixture"}
    tf._gate_pointer_reach()


def test_pointer_reach_budget_parses_the_committed_file() -> None:
    # R1650 — a ratchet whose reader silently produced nothing would allow
    # everything, which is the failure mode a budget file has: it looks like a
    # gate and behaves like an exemption for the whole tree.
    budget = _pointer_reach_budget()
    check(bool(budget), "budget: the committed file parses to something")
    check(
        all(isinstance(v, frozenset) and v for v in budget.values()),
        "budget: every example maps to a non-empty tag set",
    )
    check(
        all(not k.startswith("#") for k in budget),
        "budget: comment lines are not read as examples",
    )


def test_a_budget_file_has_four_states_and_a_missing_row_is_an_error() -> None:
    # R1662 — the three-state ratchet, plus the state R1661 did not have a name
    # for: the FILE not existing at all. Each one is a different verdict and
    # exactly one of them is a failure, which is the whole point — before
    # R1661 a missing row read as a budget of 0 (the strictest claim there is,
    # made by nobody) and before R1662 a missing file read the same way for
    # every surface at once.
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        docs = Path(tmp) / "docs"
        docs.mkdir()
        (docs / "b.tsv").write_text(
            "# a comment\n"
            + rpc_verify.CENSUS_MARKER
            + "\n"
            "hello-measured\t7\n"
            "hello-unmeasured\tunmeasured\tno sound card on this host\n",
            encoding="utf-8",
        )
        # The same file WITHOUT the census stamp: a pre-census list of the
        # non-zero examples, which is what this tree's two ratchets were until
        # a producer existed. A missing row there is the zero it was always
        # read as -- arming the strict rule against such a file fails every
        # surface it does not list, including the boot gates the producer
        # itself drives, which is a deadlock.
        (docs / "old.tsv").write_text("hello-measured\t7\n", encoding="utf-8")
        real_root = rpc_verify.WORKSPACE_ROOT
        rpc_verify.WORKSPACE_ROOT = Path(tmp)
        rpc_verify._READ_BUDGETS.clear()
        try:
            check(
                rpc_verify._budget_for("b.tsv", "hello-measured", "g", "k") == 7,
                "budget: a measured row is its number",
            )
            check(
                rpc_verify._budget_for("b.tsv", "hello-unmeasured", "g", "k") is None,
                "budget: an unmeasured row reports and does not judge",
            )
            raised = False
            try:
                rpc_verify._budget_for("b.tsv", "hello-absent", "g", "k")
            except AssertionError:
                raised = True
            check(raised, "budget: a MISSING row is an error, not a zero")
            check(
                rpc_verify._budget_for("old.tsv", "hello-absent", "g", "k3") == 0,
                "budget: a file that does not claim to be a census reads an "
                "absent row as zero, so a producer can bootstrap one",
            )
            rpc_verify._READ_BUDGETS.clear()
            check(
                rpc_verify._budget_for("nope.tsv", "hello-measured", "g", "k2") is None,
                "budget: a file that was never produced is UNARMED, not zero",
            )
            check(
                "#" not in str(rpc_verify._read_budget("b.tsv")),
                "budget: comment lines are not read as examples",
            )
        finally:
            rpc_verify.WORKSPACE_ROOT = real_root
            rpc_verify._READ_BUDGETS.clear()


def main() -> int:
    # R1580 — the population is DERIVED from this module, not listed.
    #
    # It was a hand-written tuple, which is the shape
    # [[debt-focus-sweep-population-is-curated]] is about one file over: a case
    # added and not registered does not fail, it does not exist. Definition
    # order is preserved (`globals()` is insertion-ordered), so the report reads
    # top to bottom.
    cases = [
        (name, fn)
        for name, fn in list(globals().items())
        if name.startswith("test_") and callable(fn)
    ]
    if not cases:
        print("[harness] the case scan found nothing — it is broken, not clean")
        return 1
    global CURRENT
    for name, fn in cases:
        CURRENT = name
        # R1580 — a case that RAISES is a failed case, not a dead suite. Before
        # this, an exception escaping any helper under test took the summary
        # line with it, so the other twenty-one cases reported nothing at all —
        # which is how a single broken helper hides every other verdict. Found
        # by a counterfactual: making `wait_until` consult its deadline before
        # its predicate turned this file silent instead of red.
        try:
            fn()
        except BaseException as exc:  # noqa: BLE001 — a raise IS the verdict
            check(False, f"raised {exc!r}")
    CURRENT = ""
    print(f"[harness] {len(cases)} case(s): {PASSED} passed, {len(FAILED)} failed")
    if FAILED:
        for label in FAILED:
            print(f"  - {label}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
