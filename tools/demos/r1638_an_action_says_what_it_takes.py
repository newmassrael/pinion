#!/usr/bin/env python3
"""R1638 — an action says what it takes, and the declaration is enough to call it.

R1637 made the declared surface the callable surface. It left the other half of
the contract unsaid: `$schema` published `{"path": "arrange", "type": "string",
"channel": "invoke"}` and nothing about the argument, so an agent could discover
that a verb exists and still had to read pinion's source to use it. Measured at
R1637 on one surface, 45 of 45 actions published no `args` at all.

The reference is ahead of us there and this round is where that is repaid: a
meta-method publishes each parameter's name and type, and a parameter list is
generated from the signature, so it cannot drift.

What this demo checks, and why each check discriminates:

* **(A) the declaration reaches a client.** `arg_form` and `args` are on the
  wire for the actions that declare them, and ABSENT for the ones that have not
  said. Silence and "takes nothing" are different claims and the wire keeps them
  apart — an empty `args` would have turned 487 undeclared actions into
  affirmative false statements.
* **(B) the declaration is USABLE.** The demo builds `arrange`'s payload out of
  the schema — separator from `arg_form`, segment order and vocabularies from
  `args` — and the call lands. Nothing here spells the wire form; if the
  declaration were wrong the call would be refused.
* **(C) the vocabulary is exact in both directions.** Every value the schema
  publishes for a closed argument is accepted, and a value it does not publish
  is refused. One direction alone passes against a list that is too short, the
  other against one that is too long.
* **(D) past the reference.** An argument says where its values COME FROM, not
  only its type — a closed set for a verb, and a live path for an index. The
  meta-object can do the first only for enum-typed parameters and the second not
  at all.
* **(E) the optional segment really is optional.** The short payload and the
  long one both land, which is what `optional` claims.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1638_an_action_says_what_it_takes.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_action_refused,
    run_demo,
)

EXT = "/external"
SCHEMA = f"{EXT}/$schema"

#: The verb R1631 added, R1637 published, and this round describes.
THE_VERB = "arrange"


def schema_of(tf: RpcSubprocess) -> dict[str, dict]:
    return {f["path"]: f for f in tf.query(SCHEMA)}


def build_payload(field: dict, values: dict[str, str]) -> str:
    """Build a delimited payload FROM THE DECLARATION.

    Deliberately generic: it reads the separator and the segment order off the
    schema rather than knowing anything about `arrange`. A demo that spelled
    `"align:horizontal:start"` would prove the surface works and say nothing
    about whether the declaration describes it.
    """
    assert field["arg_form"]["kind"] == "delimited", field
    sep = field["arg_form"]["separator"]
    out: list[str] = []
    for arg in field["args"]:
        if arg["name"] in values:
            out.append(values[arg["name"]])
        else:
            assert arg.get("optional"), f"{arg['name']} is required: {arg}"
            break
    return sep.join(out)


def checks(tf: RpcSubprocess) -> None:
    fields = schema_of(tf)

    # ── (A) the declaration reaches a client, and silence stays silent ───
    arrange = fields[THE_VERB]
    assert_eq(arrange["channel"], "invoke", "it is still an action")
    assert "args" in arrange, f"and it now says what it takes: {arrange}"
    assert_eq(arrange["arg_form"]["kind"], "delimited", "arrange's form")
    assert_eq(arrange["arg_form"]["separator"], ":", "and its separator")
    names = [a["name"] for a in arrange["args"]]
    assert_eq(names, ["pass", "axis", "edge_or_gap"], "in wire order")

    silent = [
        p
        for p, f in fields.items()
        if f.get("channel") == "invoke" and "args" not in f
    ]
    assert silent, "the undeclared majority is still honestly silent"
    for p in silent:
        assert "arg_form" not in fields[p], f"{p} publishes neither key"
    print(f"[demo] {len(fields) - len(silent)} fields describe their arguments, "
          f"{len(silent)} actions say nothing")

    # ── (D) an argument says where its values come from ──────────────────
    by_name = {a["name"]: a for a in arrange["args"]}
    assert_eq(by_name["pass"]["domain"]["kind"], "one_of", "a closed vocabulary")
    assert_eq(by_name["axis"]["domain"]["kind"], "one_of", "and so is the axis")
    assert by_name["edge_or_gap"].get("optional"), "the tail may be elided"
    assert not by_name["pass"].get("optional"), "a required argument is silent"
    # The live-domain half is checked on the audio surface below, because THIS
    # one has none — a count printed here would have been a vacuous zero, and
    # this round has already had one oracle that could not discriminate.

    passes = by_name["pass"]["domain"]["values"]
    axes = by_name["axis"]["domain"]["values"]
    assert_eq(sorted(passes), ["align", "distribute", "stack", "straighten"])
    assert_eq(sorted(axes), ["horizontal", "vertical"])

    # ── (B) + (C) the declaration is enough to CALL it ───────────────────
    # Two nodes are selected so an arrangement has something to move.
    tf.invoke(f"{EXT}/select", "0,1")
    landed = 0
    for pass_name in passes:
        for axis in axes:
            # `align` reads an edge and `stack` an integer gap; the demo learns
            # nothing about which from the name — it supplies a value the
            # surface accepts for either and lets the optional rule drop it
            # when the pass does not read one.
            payload = build_payload(
                arrange,
                {"pass": pass_name, "axis": axis, "edge_or_gap": "start"}
                if pass_name == "align"
                else {"pass": pass_name, "axis": axis, "edge_or_gap": "8"}
                if pass_name == "stack"
                else {"pass": pass_name, "axis": axis},
            )
            answer = str(tf.invoke(f"{EXT}/{THE_VERB}", payload))
            assert answer.startswith("moved:"), f"{payload!r} -> {answer!r}"
            landed += 1
    assert_eq(landed, len(passes) * len(axes), "every published combination lands")
    print(f"[demo] {landed} calls built from the declaration alone, all accepted")

    # ── (C) and a value the vocabulary does not publish is refused ───────
    for bad, slot in (("straightn", "pass"), ("diagonal", "axis")):
        values = {"pass": "align", "axis": "horizontal", "edge_or_gap": "start"}
        values[slot] = bad
        assert_action_refused(
            lambda p=build_payload(arrange, values): tf.invoke(f"{EXT}/{THE_VERB}", p),
            saying=bad,
        )
    print("[demo] a value outside the published set is refused, and named")

    # ── (E) the optional segment is optional ─────────────────────────────
    short = build_payload(arrange, {"pass": "distribute", "axis": "horizontal"})
    assert_eq(short.count(":"), 1, f"the tail was elided: {short!r}")
    assert str(tf.invoke(f"{EXT}/{THE_VERB}", short)).startswith("moved:")
    long = build_payload(
        arrange, {"pass": "align", "axis": "vertical", "edge_or_gap": "end"}
    )
    assert_eq(long.count(":"), 2, f"and supplied when the pass reads one: {long!r}")
    assert str(tf.invoke(f"{EXT}/{THE_VERB}", long)).startswith("moved:")
    print("[demo] the optional segment is optional in both directions")


#: The retained single-thread engine in `hello-audio-rt`, addressed by tag.
ENG = "/engine/external"


def live_domains(tf: RpcSubprocess) -> None:
    """(D) — an argument bounded by a path the SAME surface publishes.

    This is the half the reference has no equivalent for at all: a meta-method
    parameter is a type and nothing else, so "which ids may I stop?" is a
    question its introspection cannot carry. Here the answer is an address, and
    the demo follows it.
    """
    fields = {f["path"]: f for f in tf.query(f"{ENG}/$schema")}

    stop = fields["stop"]
    assert_eq(stop["arg_form"]["kind"], "scalar", "one argument, sent bare")
    dom = stop["args"][0]["domain"]
    assert_eq(dom["kind"], "values_of", "and its values live somewhere")
    assert_eq(dom["values_path"], "voices", "at a path on this same surface")

    # Follow the declaration: read the path it names, take a value from it, and
    # the call built that way lands. Nothing here knows what a clip or a voice
    # id is — both come off the wire.
    play = fields["play"]
    assert_eq(play["arg_form"]["kind"], "object", "play takes an object")
    clip_arg = next(a for a in play["args"] if a["name"] == "name")
    assert_eq(clip_arg["domain"]["kind"], "values_of")
    clips = tf.query(f"{ENG}/{clip_arg['domain']['values_path']}")
    assert clips, f"the declared path lists the answerable values: {clips!r}"
    voice = tf.invoke(f"{ENG}/play", {"name": sorted(clips)[0], "gain": 0.5})
    live = tf.query(f"{ENG}/{dom['values_path']}")
    assert any(v["id"] == voice for v in live), f"{voice} is in {live!r}"
    assert_eq(tf.invoke(f"{ENG}/stop", voice), True, "the id from the path stops it")
    # An optional argument really is optional: the same verb without them.
    bare = tf.invoke(f"{ENG}/play", {"name": sorted(clips)[0]})
    assert isinstance(bare, int), f"the required argument alone suffices: {bare!r}"
    tf.invoke(f"{ENG}/stop_all", None)

    # And the closed vocabulary on the RT surface, published from the enum it
    # is projected out of.
    rt = {f["path"]: f for f in tf.query("/external/$schema")}
    policy = rt["set_voice_policy"]["args"][0]
    assert_eq(policy["domain"]["kind"], "one_of")
    assert_eq(
        sorted(policy["domain"]["values"]), ["reject_newest", "steal_oldest"]
    )
    for value in policy["domain"]["values"]:
        tf.invoke("/external/set_voice_policy", value)
        assert_eq(tf.query("/external/voice_policy"), value, "published == accepted")
    print("[demo] an argument's values are an ADDRESS, and following it works")


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        checks(tf)
    with RpcSubprocess("hello-audio-rt", request_timeout=12.0) as tf:
        live_domains(tf)


if __name__ == "__main__":
    run_demo("R1638 — an action says what it takes", body)
