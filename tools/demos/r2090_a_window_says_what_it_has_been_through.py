#!/usr/bin/env python3
"""R2090 §5.16 §5.41 §2 #7 — **a window says what it has been through**, not
only what it is doing right now.

Every counter `scene/render_fidelity` published about presenting was a
*current* fact: `missed_in_a_row`, `broken_in_a_row`, `last_missed`,
`last_rung` are all reset the instant a frame reaches the screen. Measured in
`pinion_gpu::SurfaceHealth::presented` — it clears four fields — the effect is
that a window which broke and then recovered is **indistinguishable from one
that never broke**. One cumulative survivor existed, the rebuild count, and it
is incremented on the ladder's *heavy* rung only, so the cheap rung (the
common one) had never been counted anywhere at all.

That is a defect of the instrument, and it is the one standing between this
tree and a judgement it needs. The open defect
`debt-a-second-windows-surface-is-presented-before-it-is-configured` is
intermittent — measured on this host at roughly one event per eighty second-
window lifetimes — so deciding whether a repair helped needs a **denominator**:
how often anything went wrong at all, across a run nobody was watching. A
counter that forgets on recovery cannot supply one, and every reading a sweep
could take arrives *after* the recovery.

So R2090 gave the health record its cumulative half — `missed_total`,
`broken_total`, `reconfigured_total`, `repeated_total` — and this walk is what
holds the whole chain to it, from `SurfaceHealth` through the shell's seam and
the wire to a reader driving the assembled analysis tool.

What it asserts, and why each is a relation rather than a restatement:

* **A** — the assembled tool's board and a torn-off window both answer all
  four fields, so the cumulative half is window-scoped like the rest of the
  record rather than a process-wide total.
* **B** — the rung tally and the miss tally are computed on two independent
  paths (`MissTally::breakages` filters by `Missed::is_invalidation`;
  `RungTally` is written by the ladder), so **their sum must agree**: every
  breakage earns exactly one rung. A drift between those two is a defect
  neither one can report about itself.
* **C** — `missed_total >= missed_in_a_row` and `broken_total >=
  broken_in_a_row` at every reading: the cumulative half can never be behind
  the current half, which is what "cumulative" means when written as a check.
* **D** — across the whole run, no cumulative field ever DECREASES, while the
  "in a row" fields are free to. This is the property the old instrument could
  not have had, and it is checked at every generation for every window.

⚠ NON-VACUITY is reported rather than assumed: this host usually drives a whole
run without a single missed frame, in which case every count stays 0 and D
holds trivially. The walk prints how many readings it took and whether
anything ever moved, so a run that proved less says so — the idiom this tree
uses wherever a check depends on the host producing a condition.

⚠ A REPORTED device loss is not a failure here. It is this instrument working:
before R2088 the same event aborted the process. When it happens the walk says
so loudly and keeps its assertions on the relations, which hold whether or not
the window is presenting.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
CARD = "packet#0"
TORN = f"torn-{CARD}"

#: Tear-off / redock generations. Each one is a whole second-window surface
#: lifetime — created, configured, presented, destroyed — which is the unit
#: this defect's rate is measured in. Kept well inside the sweep's 180s budget:
#: measured on this host a generation costs well under a second.
GENERATIONS = 12

#: The cumulative half of the record. Named once, here, and every check below
#: derives its rows from this list rather than spelling the names again.
CUMULATIVE = ("missed_total", "broken_total", "reconfigured_total", "repeated_total")

#: The rung rows whose sum must equal `broken_total`. `rebuilds` is the heavy
#: rung's row (R2090 made it derived rather than a second count).
RUNGS = ("reconfigured_total", "rebuilds", "repeated_total")

CHECKS: list[str] = []

#: Every reading where a cumulative count was NOT zero, with the generation it
#: appeared in. Printed at the end: a run that saw none has proved the
#: relations on a quiet host and nothing about a busy one.
MOVED: list[str] = []

#: Windows that reported a device loss / an unconfigured surface. Not a
#: failure — see the module docstring — but a run that saw one judged a
#: different thing from a run that did not.
REPORTED: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    assert condition, what
    CHECKS.append(what)


def window_ids(app: RpcSubprocess) -> list[str]:
    resp = app.request("scene/windows", {})
    assert resp is not None, "scene/windows returned no response"
    return [w["id"] for w in resp.result["windows"]]


def invoke(app: RpcSubprocess, path: str, args: str) -> object:
    resp = app.request("scene/invoke", {"path": f"/analyzer_shell{EXT}/{path}", "args": args})
    assert resp is not None, f"{path} returned no response"
    return resp.result


def settle(app: RpcSubprocess) -> None:
    """Let the topology Effect run and the shell reconcile its windows."""
    for _ in range(8):
        app.tick_ms(16)


def health_of(app: RpcSubprocess, window: str, when: str) -> dict:
    resp = app.request("scene/render_fidelity", {"window": window})
    assert resp is not None and resp.result is not None, (
        f"{when}/{window}: scene/render_fidelity answers"
    )
    health = resp.result.get("health")
    ok(f"{when}/{window}: the frame record publishes presentability", isinstance(health, dict))
    return health


def judge(health: dict, window: str, when: str) -> None:
    """Every relation that must hold of one reading, whatever the host did."""
    for field in CUMULATIVE:
        ok(
            f"{when}/{window}: `{field}` is published as a number ({health.get(field)!r})",
            isinstance(health.get(field), int),
        )
    # C — the cumulative half can never be behind the current half.
    ok(
        f"{when}/{window}: missed_total {health['missed_total']} >= "
        f"missed_in_a_row {health['missed_in_a_row']}",
        health["missed_total"] >= health["missed_in_a_row"],
    )
    ok(
        f"{when}/{window}: broken_total {health['broken_total']} >= "
        f"broken_in_a_row {health['broken_in_a_row']}",
        health["broken_total"] >= health["broken_in_a_row"],
    )
    # A breakage is one kind of miss, so it cannot outnumber them.
    ok(
        f"{when}/{window}: broken_total {health['broken_total']} <= "
        f"missed_total {health['missed_total']}",
        health["broken_total"] <= health["missed_total"],
    )
    # B — two independent paths must agree. The miss tally filters by
    # `Missed::is_invalidation`; the rung tally is written by the ladder.
    rungs = sum(health[name] for name in RUNGS)
    ok(
        f"{when}/{window}: every breakage earned exactly one rung "
        f"({rungs} rung(s) vs broken_total {health['broken_total']})",
        rungs == health["broken_total"],
    )
    ok(
        f"{when}/{window}: `presenting` agrees with the miss count",
        health["presenting"] == (health["missed_in_a_row"] == 0),
    )
    if any(health[field] for field in CUMULATIVE):
        MOVED.append(
            f"{when}/{window}: "
            + " ".join(f"{f}={health[f]}" for f in CUMULATIVE if health[f])
        )
    reason = health.get("last_missed")
    if reason in ("unconfigured", "device_lost"):
        REPORTED.append(f"{when}/{window}: {reason}")
        print(
            f"[demo] * REPORTED {when}/{window}: {reason!r} -- the framework "
            f"NAMED a state that used to abort the process. The relations "
            f"below are judged anyway; presentability is not."
        )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        settle(app)
        # `window -> {field: last value}`, so D compares a reading against the
        # same window's previous one rather than against a literal.
        seen: dict[str, dict[str, int]] = {}

        def read(window: str, when: str) -> dict:
            health = health_of(app, window, when)
            judge(health, window, when)
            previous = seen.get(window)
            if previous is not None:
                for field in CUMULATIVE:
                    # D — the property the old instrument could not have.
                    ok(
                        f"{when}/{window}: `{field}` did not go backwards "
                        f"({previous[field]} -> {health[field]})",
                        health[field] >= previous[field],
                    )
            seen[window] = {field: health[field] for field in CUMULATIVE}
            return health

        banner("A — the board answers for its whole life, not only for now")
        assert window_ids(app) == ["main"], "the application opens with one window"
        board = read("main", "A")
        ok(
            "A: a window that has never missed a frame says so with numbers "
            "rather than with an absent key",
            all(isinstance(board[field], int) for field in CUMULATIVE),
        )

        banner(f"B — {GENERATIONS} tear-off generations, every window read every time")
        readings = 1
        for generation in range(1, GENERATIONS + 1):
            invoke(app, "act", f"{CARD},tear_off")
            settle(app)
            ids = window_ids(app)
            assert TORN in ids, f"generation {generation}: the topology grew a window"
            for window in ids:
                read(window, f"B{generation}")
                readings += 1
            invoke(app, "redock", CARD)
            settle(app)

        banner("C — and it is still answering")
        final = read("main", "C")
        ok(
            "C: the application survived the churn and still publishes a "
            "health record",
            isinstance(final["missed_total"], int),
        )

        print(f"\n[demo] {len(CHECKS)} named check(s) over {readings} reading(s)")
        print(f"[demo] windows read: {sorted(seen)}")
        print(
            "[demo] cumulative counts that moved: "
            f"{MOVED or 'none -- this run was quiet, so D held trivially'}"
        )
        print(f"[demo] framework-reported outages: {REPORTED or 'none'}")


def main() -> None:
    body()


if __name__ == "__main__":
    run_demo(SHELL, main)
