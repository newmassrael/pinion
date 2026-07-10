#!/usr/bin/env python3
"""R1269/R1270 §6.3 — async `scene/waitFor` over the single scene revision.

The v0 `scene/waitFor` busy-polls a constant scene inside one synchronous
dispatch — it can never observe a change between polls. R1269 landed the async
form; R1270 hardened it after an adversarial audit:

  * **One token** (F1): the wait keys off the *single* OCC `SceneRevision` the
    whole app shares — not a private counter. Every bump advances it: a
    dispatched mutation, shell input, AND an external-data producer's arrival
    (which now also advances the OCC token, so a preview's `base_revision`
    detects the change). The shell installs a wake observer on that one token.
  * **No lost wakeup** (L1): the park decision reads the revision *under* the
    same lock the waker drains under.
  * **Non-blocking read** (F3): `scene/revision` returns the current token so a
    client bootstraps `since` without a blind blocking call.

hello-live-data is the forcing consumer: its `Tick` pokes a producer THREAD
that appends a line and bumps the shared `SceneRevision` — a server-side wake,
so a single-threaded client can `click` then `waitFor` and be woken.

This demo drives it over RPC and verifies, all without OCR:

  (A) `scene/revision` reads the current token non-blockingly, and a dispatched
      mutation (the Tick click) advances that SAME token — proving one shared
      version, not a private waitFor counter (F1).
  (B) `waitFor { since: <before the tick> }` returns `{ changed: true,
      revision: <advanced> }` — the wait resolved (woken by the change), and
      the new line is observable in the paint scene afterwards.
  (C) The immediate-satisfaction path: `waitFor { since: 0 }` on an
      already-advanced scene answers at once with the current revision.

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
EMDASH = "—"
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


def scene_revision(d) -> int:
    """`scene/revision` — the non-blocking read of the single scene token."""
    resp = d.request("scene/revision")
    assert resp is not None, "scene/revision answered"
    rev = resp.result["revision"]
    assert isinstance(rev, int), f"revision is an int, got {rev!r}"
    return rev


def scene_wait_for(d, since: int):
    """`scene/waitFor { since }` — blocks (parks server-side) until the scene
    revision advances past `since`, then returns its `result` dict. A broken
    wake path surfaces as a request timeout, not a silent pass."""
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

        # (A) scene/revision reads the single token non-blockingly.
        boot_rev = scene_revision(d)
        assert boot_rev >= 0, "boot revision is a non-negative token"

        for i in range(1, ROUNDS + 1):
            before = scene_revision(d)
            # The Tick emits tick.click (a dispatched mutation → bumps the ONE
            # token) and pokes the producer thread, which appends a line and
            # bumps the SAME token on arrival (an external change the client did
            # not paint itself).
            d.click(path=TICK_TAG)

            # (B) Block until the scene changes. `try_async_wait_for` parks under
            # one lock (or answers immediately if the bump already landed) and
            # returns the advanced token.
            res = scene_wait_for(d, since=before)
            assert_eq(res["changed"], True, f"round {i}: waitFor reports changed")
            assert isinstance(res["revision"], int), f"round {i}: revision is an int"
            assert res["revision"] > before, f"round {i}: token advanced past {before}"

            # (A cont.) the SAME token the dispatched click advanced is the one
            # waitFor woke on — read it back, monotonic, and strictly greater
            # than the pre-tick value (a private waitFor counter bumped only by
            # the producer would NOT have moved on the click).
            after = scene_revision(d)
            assert after >= res["revision"], f"round {i}: token monotonic"
            assert after > before, f"round {i}: the dispatched click advanced the shared token"

            # (B cont.) the woken client re-reads the pane: the new line is in paint.
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

        # ── the single token advanced across the whole session (F1) ─────────
        final_rev = scene_revision(d)
        assert final_rev > boot_rev, "the ONE shared revision advanced across the session"
        final = wait_status(d, events_status(ROUNDS), "final resident pane")
        assert find_by_tag(final, LIST_TAG) is not None, "log list still present"
        assert_eq(text_under(final, row_tag(0)), producer_line(1), "line 1 resident (oldest)")
        assert_eq(text_under(final, row_tag(1)), producer_line(2), "line 2 resident")
        assert_eq(text_under(final, row_tag(2)), producer_line(3), "line 3 resident")
        assert_eq(text_under(final, row_tag(3)), producer_line(4), "line 4 resident")
        assert_eq(text_under(final, row_tag(4)), producer_line(5), "line 5 resident")
        assert_eq(text_under(final, row_tag(5)), producer_line(6), "line 6 resident (newest)")

        # ── (C) immediate-satisfaction path ────────────────────────────────
        # A baseline of 0 is stale (the scene has advanced), so this answers at
        # once with the current token — no Tick, no new line.
        cur = scene_revision(d)
        imm = scene_wait_for(d, since=0)
        assert_eq(imm["changed"], True, "immediate: changed=true")
        assert isinstance(imm["revision"], int), "immediate: revision is an int"
        assert imm["revision"] >= cur, "immediate: carries a current-or-newer token"
        snap = wait_status(d, events_status(ROUNDS), "immediate waitFor added no line")
        assert_eq(status_of(snap), events_status(ROUNDS), "a read did not poke the producer")

        # ── (D) the response is a normal JSON-RPC round-trip, id echoed ─────
        resp = d.request("scene/waitFor", {"since": 0})
        assert resp is not None, "id-addressed waitFor answered"
        assert resp.id is not None, "the response echoes the request id"
        assert isinstance(resp.result, dict), "id-addressed result is a JSON object"
        assert_eq(resp.result["changed"], True, "id-addressed reports changed")


if __name__ == "__main__":
    sys.exit(run_demo("R1270 async scene/waitFor over one revision", body))
