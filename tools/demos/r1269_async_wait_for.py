#!/usr/bin/env python3
"""R1269 PR-50 §6.3 — async `scene/waitFor`: block until the scene changes,
woken by external output the client did not cause.

The v0 `scene/waitFor` busy-polls a constant scene inside one synchronous
dispatch — it can never observe a change that arrives *between* polls, because
dispatch returns before any external state can land. This round lands the async
form: a `scene/waitFor { since: <generation> }` whose baseline is current
**parks** its one-shot reply in a `WaiterRegistry` (the dispatch thread returns
without blocking) and the embedder's external-data observer wakes it via
`WaiterRegistry::notify_changed`.

hello-live-data is the forcing consumer: its `Tick` button pokes a producer
THREAD that appends a line off-thread and — the R1269 wiring — calls
`notify_changed()` alongside its existing `request_repaint()` (the same dirty
edge, two consumers). So the wake is SERVER-side: a single-threaded RPC client
can send the poke, then a `waitFor` that parks and is woken by the producer —
exactly the "repaint on output without user input" a wire GUI needs.

This demo drives it over RPC and verifies, all without OCR:

  (A) After a `Tick`, a `waitFor { since: <prev gen> }` returns `{ changed:
      true, revision: <new> }` — the async wait resolved (parked-then-woken by
      the producer, or immediate if the notify beat the request), and the
      change generation strictly advances every round.
  (B) The woken client re-reads the pane and sees the new producer line in the
      paint scene (the data the wait announced is observable).
  (C) The immediate-satisfaction path: a `waitFor { since: 0 }` when the
      generation is already high answers at once with the current generation,
      adding no line (a read, not a poke) — same wire shape as a woken wait.
  (D) The response echoes the request id (a normal JSON-RPC round-trip, just
      fired late).

Run from the workspace root:
    cargo build -p hello-live-data --release
    python3 tools/demos/r1269_async_wait_for.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

VIEWPORT = (440, 360)
TICK_TAG = "tick"
STATUS_TAG = "log_status"
LIST_TAG = "log_list"
EMDASH = "—"  # mirrors the binding's em-dash
ROUNDS = 6  # MAX_LINES = 8, so no oldest-line eviction inside the loop


def row_tag(visible_index: int) -> str:
    return f"log_row_{visible_index}"


def producer_line(seq: int) -> str:
    """Mirror the binding's `producer_line` SSOT."""
    return f"[{seq:03d}] background event #{seq}"


def events_status(n: int) -> str:
    return "1 event" if n == 1 else f"{n} events"


def text_under(snap, tag: str):
    """The first Text content found under the container tagged `tag`."""
    node = find_by_tag(snap, tag)
    if node is None:
        return None

    def first_text(n):
        if not isinstance(n, dict):
            return None
        if n.get("type") == "Text":
            return n.get("content")
        for child in n.get("children", []) or []:
            hit = first_text(child)
            if hit is not None:
                return hit
        return None

    return first_text(node)


def status_of(snap):
    return text_under(snap, STATUS_TAG)


def wait_status(d, expected: str, where: str):
    return wait_snap(
        d,
        lambda s: status_of(s) == expected,
        viewport=VIEWPORT,
        desc=f"status == {expected!r} ({where})",
    )


def scene_wait_for(d, since: int):
    """Issue `scene/waitFor { since }` and return its `result` dict. Blocks
    (parks server-side) until the change generation advances past `since`, so
    a broken wake path surfaces as a request timeout, not a silent pass."""
    resp = d.request("scene/waitFor", {"since": since})
    assert resp is not None, "waitFor returned no response"
    assert isinstance(resp.result, dict), f"waitFor result must be an object, got {resp.result!r}"
    return resp.result


def body() -> None:
    with RpcSubprocess("hello-live-data", request_timeout=12.0) as d:
        # ── boot: empty log, list + button present ──────────────────────────
        snap = wait_status(d, f"No events yet {EMDASH} press Tick", "boot")
        assert find_by_tag(snap, LIST_TAG) is not None, "log list present at boot"
        assert find_by_tag(snap, TICK_TAG) is not None, "Tick button present at boot"
        assert_eq(status_of(snap), f"No events yet {EMDASH} press Tick", "boot status text")

        gen = 0
        for i in range(1, ROUNDS + 1):
            # (poke) The Tick emits tick.click; the producer thread appends a
            # line off-thread and calls notify_changed() — an external-output
            # analog the client did NOT paint itself.
            d.click(path=TICK_TAG)

            # (A) Block until the scene changes. The reply parks in the
            # WaiterRegistry and the producer's notify wakes it (or answers
            # immediately if the notify already landed) — either way it returns
            # the advanced generation.
            res = scene_wait_for(d, since=gen)
            assert_eq(res["changed"], True, f"round {i}: waitFor reports changed")
            assert isinstance(res["revision"], int), f"round {i}: revision is an int"
            new_gen = res["revision"]
            assert new_gen > gen, f"round {i}: generation advanced ({gen} -> {new_gen})"
            gen = new_gen

            # (B) The woken client re-reads the pane: the new line is in paint.
            snap = wait_status(d, events_status(i), f"round {i} line landed in paint")
            assert_eq(status_of(snap), events_status(i), f"round {i}: status count")
            assert_eq(
                text_under(snap, row_tag(i - 1)),
                producer_line(i),
                f"round {i}: newest producer line observable after the wait",
            )
            assert_eq(
                text_under(snap, row_tag(0)),
                producer_line(1),
                f"round {i}: oldest line stays resident (oldest-first)",
            )

        # ── full resident pane after the async rounds (oldest-first) ────────
        final = wait_status(d, events_status(ROUNDS), "final resident pane")
        assert find_by_tag(final, LIST_TAG) is not None, "log list still present"
        assert_eq(text_under(final, row_tag(0)), producer_line(1), "line 1 resident (oldest)")
        assert_eq(text_under(final, row_tag(1)), producer_line(2), "line 2 resident")
        assert_eq(text_under(final, row_tag(2)), producer_line(3), "line 3 resident")
        assert_eq(text_under(final, row_tag(3)), producer_line(4), "line 4 resident")
        assert_eq(text_under(final, row_tag(4)), producer_line(5), "line 5 resident")
        assert_eq(text_under(final, row_tag(5)), producer_line(6), "line 6 resident (newest)")
        assert gen == ROUNDS, f"one change generation per Tick: gen={gen} == {ROUNDS} ticks"

        # ── (C) immediate-satisfaction path ────────────────────────────────
        # A baseline older than the current generation answers at once with the
        # current generation — no new Tick, no new line.
        imm = scene_wait_for(d, since=0)
        assert_eq(imm["changed"], True, "immediate: changed=true")
        assert isinstance(imm["revision"], int), "immediate: revision is an int"
        assert_eq(imm["revision"], gen, "immediate: carries the current generation")
        snap = wait_status(d, events_status(ROUNDS), "immediate waitFor added no line")
        assert_eq(status_of(snap), events_status(ROUNDS), "a read did not poke the producer")

        # A baseline one below the current generation is also already stale.
        imm2 = scene_wait_for(d, since=gen - 1)
        assert_eq(imm2["changed"], True, "since=gen-1: changed=true")
        assert_eq(imm2["revision"], gen, "since=gen-1 is stale → immediate at the current gen")

        # ── (D) the response is a normal JSON-RPC round-trip, id echoed ─────
        resp = d.request("scene/waitFor", {"since": 0})
        assert resp is not None, "id-addressed waitFor answered"
        assert resp.id is not None, "the response echoes the request id"
        assert isinstance(resp.result, dict), "id-addressed result is a JSON object"
        assert_eq(resp.result["changed"], True, "id-addressed reports changed")
        assert_eq(resp.result["revision"], gen, "id-addressed immediate carries the current gen")


if __name__ == "__main__":
    sys.exit(run_demo("R1269 async scene/waitFor", body))
