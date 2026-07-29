#!/usr/bin/env python3
"""R1485 §5.12 §2 #7 — a refusal says which surface refused.

R1482 gave a `scene/query` ANSWER its provenance: `with_origin` reports
`state`, `paint_driver` or `paint_frame`, because those three surfaces answer
identically and mean three different things. It left the other half of the
outcome mute.

Measured against this same binding at HEAD, before this round:

    /external/nope        (the state scene's model refused)   UnknownIntrospectPath
    /sim/external/nope    (a live driver refused)             UnknownIntrospectPath
    /probe/external/nope  (a per-frame node refused)          UnknownIntrospectPath

Three surfaces, three refusals, one wire frame — and `with_origin:true` was
accepted and then silently ignored on the failure channel. The fact is not
derivable either: `scene/query` retries the painted frame when the state scene
has nothing at the path (R828), so `UnknownIntrospectPath` may have come from
either scene, and from either kind of node within the painted one.

This demo asserts the word arrives AND that it is checkable against facts
independent of itself:

  * a refusal names the same surface that surface's own ANSWERS name, so it
    says where the matching `$schema` lives — "no" becomes a next step;
  * that `$schema` really does lack the refused slot;
  * the origin still predicts the WRITE contract R1481 settled, on the
    failure channel exactly as R1482 showed on the success one;
  * a refusal that reached NO surface names none — absence is the report,
    not an omission, and `null` would be a different and untrue claim.

ZERO-FLAKE: the driver ticks, so nothing here compares a driver reading
against a previously-observed one. Every assertion is an origin word, a typed
refusal, a schema field list, or wire bytes — all exact.

Run from the workspace root:
    cargo build -p hello-answer-origin --release
    python3 tools/demos/r1485_refusal_origin.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    rpc_error_data,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-answer-origin"

# The primary external is the state scene's ROOT, so the `/external/`
# short-circuit is its address (R1483 also made `/model/external/...` reach it).
MODEL = "/external"
SIM = "/sim/external"
PROBE = "/probe/external"

# A slot no surface in this binding declares, so every surface refuses it for
# the same reason — which is exactly what made the three indistinguishable.
UNDECLARED = "nope"


def refusal(g, path: str, *, with_origin: bool = True):
    """Return the `error.data` of a refused `scene/query`, or fail loudly.

    `expect` mirrors the request: asking for disclosure and receiving the
    bare word IS the pre-R1485 defect, so it is reported as a shape mismatch
    naming the path rather than as a `TypeError` from the caller's first
    subscript further down.
    """
    params = {"path": path}
    if with_origin:
        params["with_origin"] = True
    data = rpc_error_data(
        lambda: g.request("scene/query", params).result,
        expect=dict if with_origin else str,
        label=f"query {path!r} (with_origin={with_origin})",
    )
    if with_origin:
        assert "reason" in data, f"{path}: a refusal always states its reason: {data}"
    return data


def body():
    with RpcSubprocess(EXAMPLE) as g:
        # ---------------------------------------------------------------
        # ★A premise — a painted frame exists, so the R828 fallback is live.
        # Without it `sim` and `probe` are simply absent and this demo would
        # be asserting against a binding it is not describing.
        # ---------------------------------------------------------------
        wait_until(
            lambda: g.query(f"{SIM}/frames") is not None,
            desc="the first paint lands",
        )
        for path in (f"{MODEL}/ticks", f"{SIM}/frames", f"{PROBE}/stamped"):
            answered = g.query(path, with_origin=True)
            assert_eq(sorted(answered.keys()), ["origin", "value"], f"{path} shape")

        answered_origin = {
            MODEL: g.query(f"{MODEL}/ticks", with_origin=True)["origin"],
            SIM: g.query(f"{SIM}/frames", with_origin=True)["origin"],
            PROBE: g.query(f"{PROBE}/stamped", with_origin=True)["origin"],
        }
        assert_eq(answered_origin[MODEL], "state", "model answers as state")
        assert_eq(answered_origin[SIM], "paint_driver", "sim answers as driver")
        assert_eq(answered_origin[PROBE], "paint_frame", "probe answers as frame")

        # ---------------------------------------------------------------
        # ★B the defect — the three refusals carried one reason word.
        # Asserted as a PREMISE first: if a future edit gave them distinct
        # reasons, the origin would no longer be what distinguishes them and
        # every assertion below would be testing a claim that had moved.
        # ---------------------------------------------------------------
        refusals = {
            surface: refusal(g, f"{surface}/{UNDECLARED}")
            for surface in (MODEL, SIM, PROBE)
        }
        for surface, data in refusals.items():
            assert_eq(
                data["reason"],
                "UnknownIntrospectPath",
                f"{surface} refuses for the shared reason",
            )

        # ---------------------------------------------------------------
        # ★C the fix — three refusals, three origins.
        # ---------------------------------------------------------------
        assert_eq(refusals[MODEL]["origin"], "state", "the model refused")
        assert_eq(refusals[SIM]["origin"], "paint_driver", "the driver refused")
        assert_eq(refusals[PROBE]["origin"], "paint_frame", "the frame node refused")
        assert_eq(
            len({d["origin"] for d in refusals.values()}),
            3,
            "three surfaces must produce three origins",
        )

        # ---------------------------------------------------------------
        # ★D checked against an independent fact — a surface reports ONE
        # identity. A refusal naming a different surface than that surface's
        # own answers would send a client to the wrong schema.
        # ---------------------------------------------------------------
        for surface in (MODEL, SIM, PROBE):
            assert_eq(
                refusals[surface]["origin"],
                answered_origin[surface],
                f"{surface} names one surface whether it answers or refuses",
            )

        # ---------------------------------------------------------------
        # ★E what the word is FOR — it addresses the `$schema` that explains
        # the refusal, and that schema really does lack the slot.
        # ---------------------------------------------------------------
        for surface in (MODEL, SIM, PROBE):
            described = g.query(f"{surface}/$schema", with_origin=True)
            assert_eq(
                described["origin"],
                refusals[surface]["origin"],
                f"{surface} $schema comes from the surface that refused",
            )
            declared = {field["path"] for field in described["value"]}
            assert UNDECLARED not in declared, (
                f"{surface} schema must not declare {UNDECLARED!r}: {declared}"
            )
            assert declared, f"{surface} must declare at least one slot"

        # ---------------------------------------------------------------
        # ★F the origin still predicts the WRITE contract (R1481/R1482), on
        # the failure channel too — so the word means the same thing in both
        # halves of the outcome rather than being a second vocabulary.
        # ---------------------------------------------------------------
        writable = {"state", "paint_driver"}
        assert refusals[MODEL]["origin"] in writable, "state is writable"
        assert refusals[SIM]["origin"] in writable, "a live driver is writable"
        assert refusals[PROBE]["origin"] not in writable, "a per-frame Box is not"
        g.intervene(f"{MODEL}/ticks", 11)
        assert_eq(g.query(f"{MODEL}/ticks"), 11, "the state write landed")
        g.intervene(f"{SIM}/speed", 7)
        assert_eq(g.query(f"{SIM}/speed"), 7, "the driver write landed")
        assert_rpc_error(
            lambda: g.intervene(f"{PROBE}/stamped", 1),
            data="NoExternalAtPath",
        )

        # ---------------------------------------------------------------
        # ★G a refusal that reached NO surface names none. Absence is the
        # report: a `null` origin would claim some unidentified surface.
        # Three different stages end the walk — scene walk, path shape, and
        # window prefix — so the absence is pinned at all three.
        # ---------------------------------------------------------------
        for path, reason in (
            ("/ghost/external/value", "NoExternalAtPath"),
            ("/sim/frames", "UnsupportedPath"),
            ("/window[]/sim/external/frames", "EmptyWindowId"),
        ):
            data = refusal(g, path)
            assert_eq(data["reason"], reason, f"{path} reason")
            assert "origin" not in data, (
                f"{path} reached no surface, so none may be named: {data}"
            )

        # ---------------------------------------------------------------
        # ★H not asking costs the ratified shape nothing — the same opt-in
        # R1482 established, now covering refusals. `error.data` stays the
        # bare string every pre-R1485 client matches on.
        # ---------------------------------------------------------------
        for surface in (MODEL, SIM, PROBE):
            bare = refusal(g, f"{surface}/{UNDECLARED}", with_origin=False)
            assert_eq(bare, "UnknownIntrospectPath", f"{surface} bare refusal")
            assert_eq(
                refusals[surface]["reason"],
                bare,
                f"{surface} disclosing form carries the bare word verbatim",
            )
        assert_eq(
            refusal(g, "/ghost/external/value", with_origin=False),
            "NoExternalAtPath",
            "an unreached refusal is bare too",
        )

        # ---------------------------------------------------------------
        # ★I the answer channel is untouched — R1482's shape survives this
        # round, so the two disclosures are one feature and not two.
        # ---------------------------------------------------------------
        assert_eq(g.query(f"{MODEL}/ticks"), 11, "the bare answer is still the value")
        assert_eq(
            json.dumps(g.query(f"{SIM}/speed")),
            "7",
            "a bare answer carries no envelope",
        )

        print("[demo] refusals now say which of the three surfaces refused")


sys.exit(run_demo("r1485-refusal-origin", body))
