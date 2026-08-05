#!/usr/bin/env python3
"""R1482 §5.12 §2 #7 §2 #2 — an answer says which surface produced it.

`scene/query` resolves against the retained state scene first and, when that
scene has nothing at the path, retries against the last painted frame (R828,
so a live game-loop driver is readable at all). What the retry reaches is not
one kind of thing, and the difference is a contract difference:

    /external/ticks          state scene       current, writable
    /sim/external/frames     live driver Rc    current, writable
    /probe/external/stamped  per-frame Box     as of the last painted frame

Before R1482 all three produced a bare `{"result": <value>}`. Measured on the
dispatcher at HEAD, the three answers were `7`, `99` and `0` — three values in
one wire shape, with nothing to say that the third one stops tracking the app
the moment painting stops, or that a write to it will be refused.

`params.with_origin` puts the provenance in the SAME answer. It has to be the
same answer: between two calls the painted frame can be replaced, so a
provenance fetched separately may describe a frame the value never came from.

The origin word is checked here against something independent of itself — it
predicts the WRITE contract R1481 settled. `state` and `paint_driver` name
surfaces a write reaches; `paint_frame` names a `Box` the next paint discards,
which `scene/intervene` refuses. An origin word that did not line up with what
the write channel actually does would be a §2 #7 lie in a new place.

ZERO-FLAKE: the driver ticks, so no assertion here compares a driver reading
against a previously-observed one except as "it moved", and no assertion
depends on when a repaint lands. Everything else is a value this demo wrote,
an origin word, or a typed refusal — all exact.

Run from the workspace root:
    cargo build -p hello-answer-origin --release
    python3 tools/demos/r1482_answer_origin.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_disclosed,
    assert_eq,
    assert_rpc_error,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-answer-origin"

# The primary external is the state scene's ROOT (measured: `scene/snapshot
# from=state` reports `{"type":"External","tag":"model"}`), so the `/external/`
# short-circuit is its address. A tagged root is not reachable by walking to
# its own tag — an addressing asymmetry noted, not fixed, by this round.
MODEL = "/external"
SIM = "/sim/external"
PROBE = "/probe/external"


def origin_of(tf: RpcSubprocess, path: str) -> str:
    """The origin word from a disclosing read, with the envelope checked.

    R1487 lifted the envelope check to `rpc_verify.assert_disclosed` once a
    third demo needed it; the shape it enforces is unchanged.
    """
    return str(assert_disclosed(tf.query(path, with_origin=True), path)["origin"])


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) premise: all three surfaces answer, through one method ──────
        # If any of them did not resolve, the origin claims below would be
        # about a smaller surface than the round is about.
        for path in (f"{MODEL}/ticks", f"{SIM}/frames", f"{PROBE}/stamped"):
            value = tf.query(path)
            assert isinstance(value, int), f"A: {path} reads {value!r}"

        # ── (B) the defect: three contracts, one wire shape ─────────────────
        # The bare form is unchanged, so this is exactly what a pre-R1482
        # client sees — three integers with nothing to tell them apart.
        bare = [tf.query(f"{MODEL}/ticks"), tf.query(f"{SIM}/frames"), tf.query(f"{PROBE}/stamped")]
        assert all(isinstance(v, int) for v in bare), f"B: bare answers {bare!r}"

        # ── (C) each answer now says what it is ────────────────────────────
        model_origin = origin_of(tf, f"{MODEL}/ticks")
        sim_origin = origin_of(tf, f"{SIM}/frames")
        probe_origin = origin_of(tf, f"{PROBE}/stamped")
        assert_eq(model_origin, "state", "★C: the state scene answered the model")
        assert_eq(sim_origin, "paint_driver", "★C: a live driver answered the sim")
        assert_eq(probe_origin, "paint_frame", "★C: a per-frame node answered the probe")
        assert_eq(
            len({model_origin, sim_origin, probe_origin}),
            3,
            "★C: three answers, three origins — the distinction that was missing",
        )

        # ── (D) a live driver is NOT lumped in with the per-frame copy ──────
        # Both come from the painted frame, so a bare "which scene answered"
        # disclosure would give them the same word. They do not have the same
        # contract: this one tracks a running simulation.
        assert_eq(
            sim_origin != probe_origin,
            True,
            "★D: the two painted-frame surfaces are told apart, not merged",
        )
        moved = wait_until(
            lambda: tf.query(f"{SIM}/frames") != tf.query(f"{SIM}/frames"),
            timeout=4.0,
            desc="the driver is being ticked",
        )
        assert moved, "★D: the paint_driver answer really does track a live loop"

        # ── (E) the origin word predicts the write contract ─────────────────
        # Checked against the independent fact R1481 settled, so the word is
        # not merely self-consistent. A write reaches the two `current`
        # origins; the as-of-frame one is refused rather than silently lost.
        tf.intervene(f"{MODEL}/ticks", 41)
        assert_eq(tf.query(f"{MODEL}/ticks"), 41, "★E: a `state` answer names a writable surface")
        tf.intervene(f"{SIM}/speed", 7)
        assert_eq(
            tf.query(f"{SIM}/speed"), 7, "★E: a `paint_driver` answer names a writable surface"
        )
        # R1487 — the refusal is the same one R1481 settled; the WORD is not.
        # It used to be `NoExternalAtPath`, which claimed there was nothing at
        # an address this demo reads two lines below. It now says what is
        # actually true of the surface it reached.
        assert_rpc_error(
            lambda: tf.intervene(f"{PROBE}/stamped", 7),
            data="RetainedNodeNotWritable",
        )
        assert_eq(
            tf.query(f"{PROBE}/stamped") is not None,
            True,
            "★E: …and the refusal did not make the path unreadable",
        )

        # ── (F) the write did not move the answer's origin ──────────────────
        # A surface that changed which scene answers it after a write would
        # make the word describe the past.
        assert_eq(origin_of(tf, f"{MODEL}/ticks"), "state", "F: the model still answers as state")
        assert_eq(
            origin_of(tf, f"{SIM}/speed"),
            "paint_driver",
            "F: the driver still answers as paint_driver",
        )

        # ── (G) not asking costs the existing shape nothing ─────────────────
        # ~287 call sites read `result` as the value itself. The disclosure is
        # opt-in so that stays exactly true — including for the fallback
        # answers, which are the ones that changed code path.
        for path in (f"{MODEL}/ticks", f"{SIM}/frames", f"{PROBE}/stamped"):
            bare_value = tf.query(path)
            disclosed = tf.query(path, with_origin=True)
            assert not isinstance(bare_value, dict), f"★G: {path} bare answer is still the value"
            assert isinstance(disclosed, dict), f"G: {path} disclosing answer is an object"
            assert "value" in disclosed, f"G: {path} envelope carries the value"
        # The model is the one surface whose value cannot move on its own, so
        # it is the one where the two shapes are exactly comparable.
        assert_eq(
            tf.query(f"{MODEL}/ticks"),
            assert_disclosed(
                tf.query(f"{MODEL}/ticks", with_origin=True), f"{MODEL}/ticks"
            )["value"],
            "★G: the wrapped value is the same answer, not a different read",
        )

        # ── (H) discovery reports the origin of the contract it described ───
        # An agent that discovers a schema through the fallback needs to know
        # whether the contract it found belongs to a live surface.
        for path, expected in (
            (f"{MODEL}/$schema", "state"),
            (f"{SIM}/$schema", "paint_driver"),
            (f"{PROBE}/$schema", "paint_frame"),
        ):
            schema = tf.query(path)
            assert isinstance(schema, list) and schema, f"H: {path} reads {schema!r}"
            assert_eq(origin_of(tf, path), expected, f"★H: {path} reports its own origin")

        # ── (I) every declared slot answers with its surface's origin ───────
        # Walked from each surface's OWN schema, so adding a slot cannot make
        # this drift into testing less than the whole surface.
        checked = 0
        for base, expected in ((MODEL, "state"), (SIM, "paint_driver"), (PROBE, "paint_frame")):
            declared = [f["path"] for f in tf.query(f"{base}/$schema")]
            assert declared, f"I: {base} declares at least one slot"
            for slot in declared:
                assert_eq(
                    origin_of(tf, f"{base}/{slot}"),
                    expected,
                    f"★I: {base}/{slot} reports the origin of the surface holding it",
                )
                checked += 1
        assert_eq(checked >= 4, True, f"I: the sweep covered every declared slot ({checked})")

        # ── (J) the disclosure does not invent answers ──────────────────────
        # Asking for the origin must not turn a refusal into a reply: a path
        # that is nowhere is still nowhere, and an unknown slot on a real
        # surface still reads as unknown rather than as an empty envelope.
        #
        # R1485 widened what a REFUSAL carries under this same opt-in: the
        # reason word moved from `error.data` to `error.data.reason`, and a
        # refusal that reached a surface now names it. The claim here is
        # unchanged and so are the reason words — which is the point, and why
        # they are still asserted verbatim. `/nosuchtag/...` reached nothing,
        # so it names nothing; `probe` is a node the paint scene really holds,
        # so it does. (The bare — non-`with_origin` — refusal shape is
        # untouched; `r1485_refusal_origin.py` asserts those bytes.)
        assert_rpc_error(
            lambda: tf.query("/nosuchtag/external/x", with_origin=True),
            data={"reason": "NoExternalAtPath"},
        )
        assert_rpc_error(
            lambda: tf.query(f"{PROBE}/ghost", with_origin=True),
            data={"reason": "UnknownIntrospectPath", "origin": "paint_frame"},
        )
        assert_rpc_error(
            lambda: tf.query(f"{SIM}/ghost", with_origin=True),
            data={"origin": "paint_driver", "reason": "UnknownIntrospectPath"},
        )

        # ── (K) an explicit false is the same as absent ─────────────────────
        # A client threading the flag from config must not get a second shape
        # by passing the default.
        off = tf.request("scene/query", {"path": f"{MODEL}/ticks", "with_origin": False})
        assert off is not None and not isinstance(off.result, dict), (
            f"★K: with_origin=false is the bare shape, got {off.result!r}"
        )
        assert_eq(off.result, tf.query(f"{MODEL}/ticks"), "K: …and the same value")

        # ── (L) the state scene is still consulted first ────────────────────
        # The fallback must not have become the first thing tried: the model
        # is reachable in both a snapshot of the state scene and the read,
        # and its origin says the state scene is what answered.
        state = tf.snapshot(source="state")
        assert isinstance(state, dict), f"L: the state scene still answers, got {type(state)}"
        assert_eq(
            origin_of(tf, f"{MODEL}/ticks"),
            "state",
            "★L: a path present in the state scene is answered there, not by the frame",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1482 answer origin", body))
