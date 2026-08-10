#!/usr/bin/env python3
"""R1487 §5.12 §2 #7 §2 #2 — the write channel says which surface it reached.

R1482 gave a `scene/query` ANSWER its provenance and R1485 gave its REFUSAL
one, because this binding's three surfaces answer and refuse identically while
meaning three different things. The write channel was left mute on both.

Measured against this same binding at HEAD, before this round:

    invoke /external/nope        (the state model refused)  UnknownInvokePath
    invoke /sim/external/nope    (a live driver refused)    UnknownInvokePath
    invoke /probe/external/nope  (a per-frame node refused) NoExternalAtPath

    ...and `with_origin:true` was accepted and silently ignored: the frames
    with and without the flag were byte-identical, on success and on refusal.

The third line is the sharper defect. `scene/query /probe/external/stamped`
answers, and R1482 makes it name its surface: `paint_frame`. The write channel,
asked about that same address, replied "there is no external at that path" —
a statement about the scene that the read had just disproved. R1481 wrote the
rule ("a readable path may not be write-denied") and applied it to the driver;
the other painted node kind kept the false word, and R1485's own demo pinned
that word as expected behaviour.

Both halves land here:

  * an action or a write reports the surface that took it, in the same
    envelope and the same three words `scene/query` uses;
  * a refusal reports the surface it reached, and the per-frame refusal says
    `RetainedNodeNotWritable` — the refusal R1481 intended, without the claim
    that nothing is there;
  * a refusal that reached NO surface names none, so absence stays the report;
  * not asking costs the ratified shape nothing, on either method.

Every origin claim is cross-checked against a fact independent of itself: what
the action DID (each surface counts its own runs, and that count is queryable),
and what the READ says about the same address.

ZERO-FLAKE: the driver ticks, so nothing here compares a driver reading against
a previously-observed one. `frames` is never asserted. Every assertion is an
origin word, a typed refusal, a value this demo itself wrote, or a run count
that only this demo can advance.

Run from the workspace root:
    cargo build -p hello-answer-origin --release
    python3 tools/demos/r1487_write_origin.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_disclosed,
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

# Each surface's own action, and — R1637 — the SEPARATE slot reporting how many
# times it has run. One name used to carry both, which a schema field cannot
# state: it declares one channel, so whichever was declared hid the other.
ACTION = {MODEL: "bump", SIM: "boost", PROBE: "restamp"}
TALLY = {MODEL: "bumps", SIM: "boosts", PROBE: "restamps"}

# An action no surface declares, so each refuses it for a reason of its own.
UNDECLARED = "nope"


def refusal(g, method: str, path: str, *, with_origin: bool = True):
    """Return the `error.data` of a refused write, or fail loudly.

    `expect` mirrors the request: asking for disclosure and receiving the bare
    word IS the pre-R1487 defect, so it is reported as a shape mismatch naming
    the call rather than as a `TypeError` from a subscript further down.
    """
    params: dict = {"path": path}
    params["args" if method == "scene/invoke" else "value"] = None
    if with_origin:
        params["with_origin"] = True
    data = rpc_error_data(
        lambda: g.request(method, params).result,
        expect=dict if with_origin else str,
        label=f"{method} {path!r} (with_origin={with_origin})",
    )
    if with_origin:
        assert "reason" in data, f"{path}: a refusal always states its reason: {data}"
    return data


def body():
    with RpcSubprocess(EXAMPLE) as g:
        # ---------------------------------------------------------------
        # ★A premise — a painted frame exists, so both write fallbacks are
        # live, and the READ still reports the three origins R1482/R1485
        # settled. Everything below is stated relative to those three words;
        # if they moved, this demo would be describing a binding it is not.
        # ---------------------------------------------------------------
        wait_until(
            lambda: g.query(f"{SIM}/frames") is not None,
            desc="the first paint lands",
        )
        read_origin = {
            surface: assert_disclosed(
                g.query(f"{surface}/{slot}", with_origin=True), f"read {surface}"
            )["origin"]
            for surface, slot in ((MODEL, "ticks"), (SIM, "speed"), (PROBE, "stamped"))
        }
        assert_eq(read_origin[MODEL], "state", "the model answers as state")
        assert_eq(read_origin[SIM], "paint_driver", "the sim answers as driver")
        assert_eq(read_origin[PROBE], "paint_frame", "the probe answers as frame")

        # ---------------------------------------------------------------
        # ★B an action says which surface acted — and the word is checked
        # against what the action DID, read back through a separate call.
        # ---------------------------------------------------------------
        g.intervene(f"{MODEL}/ticks", 100)
        bumps_before = g.query(f"{MODEL}/{TALLY[MODEL]}")
        acted = assert_disclosed(
            g.invoke(f"{MODEL}/{ACTION[MODEL]}", 5, with_origin=True), "model action"
        )
        assert_eq(acted["origin"], "state", "the model acted")
        assert_eq(acted["value"], 105, "the action returned the new value")
        assert_eq(g.query(f"{MODEL}/ticks"), 105, "and the state scene holds it")
        assert_eq(
            g.query(f"{MODEL}/{TALLY[MODEL]}"),
            bumps_before + 1,
            "the surface named as `state` is the surface that ran the action",
        )

        g.intervene(f"{SIM}/speed", 20)
        boosts_before = g.query(f"{SIM}/{TALLY[SIM]}")
        acted = assert_disclosed(
            g.invoke(f"{SIM}/{ACTION[SIM]}", 3, with_origin=True), "driver action"
        )
        assert_eq(acted["origin"], "paint_driver", "the live driver acted")
        assert_eq(acted["value"], 23, "the action returned the new value")
        assert_eq(g.query(f"{SIM}/speed"), 23, "and the driver holds it")
        assert_eq(
            g.query(f"{SIM}/{TALLY[SIM]}"),
            boosts_before + 1,
            "the surface named as `paint_driver` is the one that ran the action",
        )

        # ---------------------------------------------------------------
        # ★C a write says which surface took it. The same envelope, with a
        # `null` value: `scene/intervene` has no result to report, so the
        # origin is the whole point of asking.
        # ---------------------------------------------------------------
        written = assert_disclosed(
            g.intervene(f"{MODEL}/ticks", 7, with_origin=True), "model write"
        )
        assert_eq(written["value"], None, "a write reports no value")
        assert_eq(written["origin"], "state", "the model took the write")
        assert_eq(g.query(f"{MODEL}/ticks"), 7, "and the write landed")

        written = assert_disclosed(
            g.intervene(f"{SIM}/speed", 9, with_origin=True), "driver write"
        )
        assert_eq(written["origin"], "paint_driver", "the driver took the write")
        assert_eq(g.query(f"{SIM}/speed"), 9, "and the write landed")

        # ---------------------------------------------------------------
        # ★D the defect's first half — three write refusals, three facts.
        # Pre-R1487 the first two were byte-identical and the third made a
        # false claim about the scene.
        # ---------------------------------------------------------------
        refusals = {
            surface: refusal(g, "scene/invoke", f"{surface}/{UNDECLARED}")
            for surface in (MODEL, SIM, PROBE)
        }
        # Stated as a group before anything reads an origin out, so a build
        # that stopped naming surfaces is reported as that rather than as a
        # `KeyError` from the first subscript below — the same failure mode
        # `assert_disclosed` exists for on the success channel.
        for surface, data in refusals.items():
            assert "origin" in data, (
                f"★D: {surface} is a surface the walk reached, so its refusal "
                f"must name it: {data}"
            )
        assert_eq(refusals[MODEL]["reason"], "UnknownInvokePath", "the model refused")
        assert_eq(refusals[MODEL]["origin"], "state", "and named itself")
        assert_eq(refusals[SIM]["reason"], "UnknownInvokePath", "the driver refused")
        assert_eq(refusals[SIM]["origin"], "paint_driver", "and named itself")
        # `.get` rather than `[...]`: an absent origin is a legitimate VALUE
        # of this comparison (it is what an unreached refusal reports), and
        # subscripting here would turn "one surface stopped naming itself"
        # into a `KeyError` raised before ★E can name which claim failed —
        # measured, by reverting each half of this round in turn.
        assert_eq(
            len({(d["reason"], d.get("origin")) for d in refusals.values()}),
            3,
            "three surfaces must produce three distinguishable refusals",
        )

        # ---------------------------------------------------------------
        # ★E the sharp half — the per-frame node is refused BY NAME, not by
        # denying it exists. Checked against the read of the same address:
        # the two channels now agree about what the scene contains.
        # ---------------------------------------------------------------
        assert_eq(
            refusals[PROBE]["reason"],
            "RetainedNodeNotWritable",
            "a per-frame node is refused for being one",
        )
        assert_eq(
            refusals[PROBE]["origin"],
            read_origin[PROBE],
            "and the refusal names the surface the READ names",
        )
        for surface in (MODEL, SIM, PROBE):
            assert_eq(
                refusals[surface]["origin"],
                read_origin[surface],
                f"{surface} names one surface whether it reads, acts or refuses",
            )

        # The rule, over every address this binding exposes: an address the
        # read resolves is an address the write names a surface for. One
        # walk, one answer — a per-address sweep rather than one example, so
        # a future surface cannot slip back into denying its own existence.
        checked = 0
        for surface in (MODEL, SIM, PROBE):
            for field in g.query(f"{surface}/$schema"):
                path = f"{surface}/{field['path']}"
                # The premise of each sweep step, and it must be the read
                # that establishes it: `query` raises if it cannot resolve
                # the address, so reaching the loop body IS the assertion
                # that this address is readable.
                g.query(path)
                for method in ("scene/invoke", "scene/intervene"):
                    try:
                        g.request(
                            method,
                            {
                                "path": path,
                                ("args" if method == "scene/invoke" else "value"): None,
                            },
                        )
                    except Exception as exc:  # noqa: BLE001 - the message IS the assertion
                        assert "NoExternalAtPath" not in str(exc), (
                            f"★E: {path} is readable, so {method} may not deny "
                            f"it exists: {exc}"
                        )
                    checked += 1
        assert checked >= 12, f"★E: the sweep must be non-trivial, ran {checked}"

        # ---------------------------------------------------------------
        # ★F what the refusal is NOT saying — the surface can act. `probe`
        # declares `restamp` in its own `$schema`, so the wire's refusal is
        # a fact about the route (a `Box` the next paint discards), not
        # about a missing capability. Without this the new word would be
        # indistinguishable from "this surface has no such action".
        # ---------------------------------------------------------------
        declared = {f["path"] for f in g.query(f"{PROBE}/$schema")}
        assert ACTION[PROBE] in declared, f"probe must declare its action: {declared}"
        data = refusal(g, "scene/invoke", f"{PROBE}/{ACTION[PROBE]}")
        assert_eq(
            data["reason"],
            "RetainedNodeNotWritable",
            "a DECLARED action on a per-frame node is refused the same way",
        )
        assert_eq(data["origin"], "paint_frame", "and by the same surface")
        assert_eq(
            g.query(f"{PROBE}/{TALLY[PROBE]}"),
            0,
            "the refusal was real: the action never ran",
        )
        assert_eq(
            refusal(g, "scene/intervene", f"{PROBE}/stamped")["reason"],
            "RetainedNodeNotWritable",
            "the write channel refuses it the same way as the action channel",
        )

        # ---------------------------------------------------------------
        # ★G a refusal that reached NO surface names none. Absence is the
        # report: a `null` origin would claim some unidentified surface.
        # Three different stages end the walk, so it is pinned at all three.
        # ---------------------------------------------------------------
        for path, reason in (
            ("/ghost/external/value", "NoExternalAtPath"),
            ("/sim/frames", "UnsupportedPath"),
            ("/window[]/sim/external/speed", "EmptyWindowId"),
        ):
            for method in ("scene/invoke", "scene/intervene"):
                data = refusal(g, method, path)
                assert_eq(data["reason"], reason, f"{method} {path} reason")
                assert "origin" not in data, (
                    f"{method} {path} reached no surface, so none may be "
                    f"named: {data}"
                )

        # ---------------------------------------------------------------
        # ★H not asking costs the ratified shape nothing — the opt-in R1482
        # established, now on two more methods, success and refusal alike.
        # ---------------------------------------------------------------
        assert_eq(
            json.dumps(g.invoke(f"{MODEL}/{ACTION[MODEL]}", 0)),
            "7",
            "a bare action result carries no envelope",
        )
        assert_eq(
            g.intervene(f"{MODEL}/ticks", 7),
            None,
            "a bare write still results in null",
        )
        for surface in (MODEL, SIM):
            bare = refusal(g, "scene/invoke", f"{surface}/{UNDECLARED}", with_origin=False)
            assert_eq(bare, "UnknownInvokePath", f"{surface} bare refusal")
            assert_eq(
                refusals[surface]["reason"],
                bare,
                f"{surface} disclosing form carries the bare word verbatim",
            )
        assert_rpc_error(
            lambda: g.invoke(f"{PROBE}/{ACTION[PROBE]}", None),
            data="RetainedNodeNotWritable",
        )

        # ---------------------------------------------------------------
        # ★I one vocabulary, three methods — the point of reusing the word
        # rather than minting a second one. What a caller learns from a read
        # predicts what the write will report, which is what makes the
        # origin actionable rather than decorative.
        # ---------------------------------------------------------------
        writable = {"state", "paint_driver"}
        assert read_origin[MODEL] in writable, "state is writable"
        assert read_origin[SIM] in writable, "a live driver is writable"
        assert read_origin[PROBE] not in writable, "a per-frame Box is not"
        assert_eq(
            assert_disclosed(
                g.invoke(f"{MODEL}/{ACTION[MODEL]}", 0, with_origin=True), "action"
            )["origin"],
            read_origin[MODEL],
            "read and action agree on the state surface",
        )
        assert_eq(
            assert_disclosed(
                g.intervene(f"{SIM}/speed", 9, with_origin=True), "write"
            )["origin"],
            read_origin[SIM],
            "read and write agree on the driver surface",
        )

        # ---------------------------------------------------------------
        # ★J the read channel is untouched — R1482 and R1485 survive this
        # round, so the three disclosures are one feature and not three.
        # ---------------------------------------------------------------
        assert_eq(g.query(f"{MODEL}/ticks"), 7, "the bare answer is still the value")
        assert_eq(
            rpc_error_data(
                lambda: g.query(f"{SIM}/{UNDECLARED}", with_origin=True),
                expect=dict,
                label="query refusal",
            ),
            {"reason": "UnknownIntrospectPath", "origin": "paint_driver"},
            "R1485's refusal disclosure is unchanged",
        )

        print("[demo] writes now say which of the three surfaces acted or refused")


sys.exit(run_demo("r1487-write-origin", body))
