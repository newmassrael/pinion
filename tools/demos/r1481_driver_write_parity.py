#!/usr/bin/env python3
"""R1481 §2 #4 §5.12 §2 #2 — a path the read answers, the write may not deny.

`hello-immediate-intent` runs a bouncing-ball driver as a `Scene::
ImmediateModeNode`. R828 gave `scene/query` a paint-scene fallback so an agent
could read that driver's live state — a driver exists ONLY in the painted
frame, never in the boot-frozen state scene. `scene/invoke` and
`scene/intervene` never got the same fallback, and R828 said why: they handed
back a `&mut dyn ExternalIntrospect` that had to outlive resolution, which a
`RefCell` borrow cannot do. The deferral was explicitly conditioned on "until
a consumer needs it".

The consumer arrived as a defect, measured on this binding:

    scene/query     ball/external/velocity  ->  -2.5
    scene/intervene ball/external/velocity  ->  error NoExternalAtPath
    scene/invoke    ball/external/reset     ->  error NoExternalAtPath

"There is no external at that path" — which the read had just disproved. The
driver declares `introspect_mut`, so the write channel existed and was simply
unreachable over the wire: read-only by accident, not by design
([[wire-form-read-write-symmetry]]).

R1481 dissolves the borrow seam the way the read already did — do the work
INSIDE the borrow and return an owned value — so a shared `&Scene` suffices,
which is what lets the painted frame answer a write at all. A retained
`ExternalNode` in a painted frame is still refused, and that is the truthful
answer rather than a shortcut: it is a `Box` the view fn rebuilds every frame,
so a write into it would vanish before the next paint.

ZERO-FLAKE: nothing here waits on wall-clock or pixels. The ball ticks, so the
demo never asserts a position — it asserts what a WRITE did (a value it chose,
read back) and what an ACTION returned, both of which are exact. `bounces` is
only ever compared as "did not go backwards", which a tick cannot break.

Run from the workspace root:
    cargo build -p hello-immediate-intent --release
    python3 tools/demos/r1481_driver_write_parity.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-immediate-intent"
BALL = "ball/external"


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) premise: the read really does reach a live driver ───────────
        # If the driver were not ticking, every "the write survived a tick"
        # claim below would pass for the wrong reason.
        vel = tf.query(f"{BALL}/velocity")
        assert isinstance(vel, (int, float)), f"A: velocity reads {vel!r}"
        assert vel != 0, "A: a still ball would make the tick claims vacuous"
        assert_eq(
            tf.query(f"{BALL}/pos") is not None,
            True,
            "A: the driver answers reads at all",
        )
        moved = wait_until(
            lambda: tf.query(f"{BALL}/pos") != tf.query(f"{BALL}/pos"),
            timeout=4.0,
            desc="the ball is being ticked",
        )
        assert moved, "★A: the driver is live — reads track a running simulation"

        # ── (B) the write channel the driver declared is now reachable ──────
        # Pre-R1481 this call answered `NoExternalAtPath`: the refusal the
        # read on the very same path had already disproved.
        #
        # ZERO-FLAKE (R1485 root-cause): the SIGN is not asserted, for the
        # same reason (A) refuses to assert `pos`. A tick that crosses a wall
        # reflects the ball and sets `vel = ±|vel|`, so a bounce landing
        # between this write and the read below legitimately returns the
        # negation of what was written — observed once as `0.125` written and
        # `-0.125` read. What a bounce can never do is change the MAGNITUDE:
        # reflection preserves it, and nothing else in `tick` touches `vel`.
        # The magnitude is therefore the witness that measures the write and
        # only the write; asserting the signed value measured the write AND
        # the simulation's timing, which is why it raced.
        chosen = 0.125
        assert abs(vel) != chosen, (
            f"B premise: the driver must not already hold |{chosen}| ({vel}), "
            "or the write would be invisible"
        )
        tf.intervene(f"{BALL}/velocity", chosen)
        assert_eq(
            abs(tf.query(f"{BALL}/velocity")),
            chosen,
            "★B: the write reached the driver the read reports on",
        )

        # A second write, to a different magnitude, so (B) cannot be a fixture
        # that happened to already hold `chosen`.
        tf.intervene(f"{BALL}/velocity", -0.25)
        assert_eq(
            abs(tf.query(f"{BALL}/velocity")), 0.25, "★B: and it is not a one-off"
        )

        # ── (C) the write landed on the LIVE driver, not a per-frame copy ───
        # The distinction the whole round turns on. A copy would be replaced
        # by the next paint; this magnitude survives many of them. Sign flips
        # on a bounce, so magnitude is the invariant — asserting the signed
        # value here would be asserting the absence of a bounce, which is a
        # race, not a fact.
        for _ in range(3):
            tf.query(f"{BALL}/pos")
        assert_eq(
            abs(tf.query(f"{BALL}/velocity")),
            0.25,
            "★C: the value survives the frames that would discard a copy",
        )

        # ── (D) the action channel answers on the same live driver ──────────
        before = int(tf.query(f"{BALL}/bounces"))
        assert before >= 0, "D: bounce count is a count"
        cleared = tf.invoke(f"{BALL}/reset", None)
        assert_eq(
            cleared,
            before,
            "★D: the action reports what it cleared, from the live count",
        )
        after = int(tf.query(f"{BALL}/bounces"))
        assert after < before or before == 0, (
            f"★D: reset must have cleared the count (before={before}, after={after})"
        )
        # NOT asserted: the position `reset` writes. The ball keeps ticking
        # between the action and the read, so `pos == 0.5` is a race and would
        # be a flake dressed as a fact. What IS exact is the value the action
        # returned and the count it cleared, both asserted above.
        assert 0.0 <= float(tf.query(f"{BALL}/pos")) <= 1.0, (
            "D: the position stays in the driver's declared range after a reset"
        )
        second = tf.invoke(f"{BALL}/reset", None)
        assert_eq(
            int(second) < max(before, 1),
            True,
            "★D: a second reset clears a count the first one had already cut",
        )

        # ── (E) refusals stay typed and distinct ────────────────────────────
        # A write channel that answered everything would be worse than one
        # that answered nothing. "You cannot write this" and "this does not
        # exist" are different facts and must read differently (§2 #7).
        assert_rpc_error(
            lambda: tf.intervene(f"{BALL}/bounces", 3), data="ReadOnly"
        )
        assert_rpc_error(
            lambda: tf.intervene(f"{BALL}/ghost", 3), data="UnknownIntervenePath"
        )
        assert_rpc_error(
            lambda: tf.invoke(f"{BALL}/ghost", None), data="UnknownInvokePath"
        )
        assert_rpc_error(
            lambda: tf.intervene(f"{BALL}/velocity", "not a number"),
            data="InterveneTypeMismatch",
        )

        # ── (F) a path that reaches no driver is still refused ──────────────
        # The fallback is not a wildcard: it does not make every painted node
        # writable, only the ones whose handle the tick loop shares.
        assert_rpc_error(
            lambda: tf.intervene("nosuchtag/external/x", 1), data="NoExternalAtPath"
        )
        assert_rpc_error(
            lambda: tf.invoke("nosuchtag/external/x", None), data="NoExternalAtPath"
        )

        # ── (G) read/write symmetry: every readable slot answers a write ────
        # …with a REASON, never with "there is nothing here". That was the
        # defect: an answer about the scene's shape that contradicted the
        # read. Walk the driver's own declared schema so this cannot drift
        # as slots are added.
        schema = tf.query(f"{BALL}/$schema")
        assert isinstance(schema, list) and schema, f"G: schema reads {schema!r}"
        declared = [f["path"] for f in schema]
        assert "velocity" in declared, "G: the writable slot is declared"
        assert "bounces" in declared, "G: a read-only slot is declared too"
        answered = 0
        for path in declared:
            try:
                tf.intervene(f"{BALL}/{path}", 0)
            except Exception as exc:  # noqa: BLE001 - the message is the assertion
                assert "NoExternalAtPath" not in str(exc), (
                    f"★G: '{path}' is readable, so a write must not deny it exists: {exc}"
                )
            answered += 1
        assert_eq(
            answered,
            len(declared),
            "★G: every declared slot answered the write channel with a verdict",
        )
        assert answered > 0, "G: the sweep was not empty"
        assert_eq(
            len(declared) >= 6,
            True,
            "G: the sweep covered the driver's whole declared surface",
        )

        # ── (H) the retained half of the SAME binding is untouched ──────────
        # The fallback fires only after the state scene reports no external,
        # so a retained widget must answer exactly as it did before. This
        # binding has one (its Button), which makes the check free.
        state = tf.snapshot(source="state")
        assert state is not None, "H: the state scene still answers"
        assert isinstance(state, dict), f"H: the snapshot is a node, got {type(state)}"
        # The retained widget's own external still refuses a driver path, so
        # the fallback did not become the first thing tried.
        assert_rpc_error(
            lambda: tf.intervene("/external/velocity", 1.0), data="UnknownIntervenePath"
        )

        # ── (I) the binding is still alive after all of it ──────────────────
        assert tf.query(f"{BALL}/pos") is not None, "I: the driver still answers"
        assert isinstance(tf.query(f"{BALL}/velocity"), (int, float)), (
            "★I: reads and writes left the driver in a readable state"
        )
        assert_eq(
            int(tf.query(f"{BALL}/bounces")) >= 0,
            True,
            "I: the simulation kept running through every write",
        )
        # And the write channel still works at the end, so nothing above
        # left the driver borrowed or wedged — the `RefCell` discipline this
        # round rests on would show up here as a panic, not a wrong value.
        tf.intervene(f"{BALL}/velocity", 1.5)
        assert_eq(
            abs(tf.query(f"{BALL}/velocity")),
            1.5,
            "★I: the borrow was released every time — the driver is still writable",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1481 a read-answered path may not be write-denied", body))
