#!/usr/bin/env python3
"""R1618 §5.36 §2 #7 — a node that paints from ONE declaration still says why.

R1615 gave content made of many parts a way to name the run that decided each
part, and closed the mechanism half of
`debt-assembled-state-is-published-by-one-binding`. The coverage half would not
pay, and the reason was not effort: the mechanism reached `Text` and `TextGrid`
only. Everything else a binding assembles — a grid row, a selected cell, a
dimmed port, a colour pad — is a filled rectangle, whose marks channel is
`Uniform`, and `Uniform` was read as "there is nothing to attribute".

It is the opposite. "The node itself IS the run" is a statement about the SHAPE
of the attribution, not about its absence: a rectangle's colour is routinely a
composition — selected, and hovered, and inside a collapsed group — and those
facts live in different externals, so the composed answer belongs to no single
oracle. `domain::NODE` is that index space: one position, the node.

The colour picker is the forcing consumer, and it is the honest one. Its
saturation/value pad's BASE COLOUR is the hue held by the hue slider, while its
THUMB is the saturation and value held by the pad itself. Two externals, one
painted node, and until now a client asking "why is this pad this colour" got
half an answer from either external and had to compose the other half itself —
which means re-implementing the framework's composition on the client side.

What this script checks, and why each check discriminates:

* **The pad publishes both reasons, over the node domain.** Not a colour: the
  names survive, so "the base is the hue slider's" is readable rather than
  inferred from a pixel.
* **The stack is ordered, and the order is the resolution rule.** Declaration
  order, last wins — the same rule positional marks follow, because it is the
  same mechanism rather than a second one that could disagree.
* **One vocabulary for both shapes.** The same `scene/marks` method answers a
  whole-node attribution and a positional one, and the reply says which index
  space it counted in. A client that assumed one would read a plausible wrong
  answer, which is the reason a domain is stated at all.
* **`silent` and `no channel` stay different answers.** A node that publishes
  nothing is one nobody asked; a node that cannot publish says which KIND of
  nothing it is. Collapsing them would make "this row is plain" and "this
  binding never reports" one answer.
* **The reasons track the state.** Driving the hue slider moves the pad's base
  colour, and the published reason for that base is still the hue's — the
  attribution is derived each frame, not a label written once.

Run from the workspace root:
    cargo build -p hello-color-picker --release
    python3 tools/demos/r1618_a_node_says_why_it_is_that_colour.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    call,
    run_demo,
)

SV_TAG = "sv_pad"
HUE_TAG = "hue_bar"

#: Mirrored from the binding rather than imported — a demo that read its
#: expected answers out of the code under test could not catch that code
#: changing.
MARK_HUE_BASE = "hue_base"
MARK_SV_THUMB = "sv_thumb"
NODE_DOMAIN = "node"


def marks(tf: RpcSubprocess, tag: str, index: int | None = None) -> dict:
    params: dict = {"tag": tag}
    if index is not None:
        params["index"] = index
    return call(tf, "scene/marks", params)


def run(tf: RpcSubprocess) -> None:
    # ---- 0. the method is discoverable ----------------------------------
    catalogue = {m["name"] for m in call(tf, "rpc/methods")["methods"]}
    assert "scene/marks" in catalogue, (
        "an agent finds the attribution surface through rpc/methods, not by "
        "knowing the name in advance"
    )

    # ---- 1. the pad publishes, over the NODE domain ----------------------
    pad = marks(tf, SV_TAG)
    assert_eq(pad["tag"], SV_TAG)
    assert_eq(
        pad["published"],
        True,
        "the pad is where two externals meet, and it says so",
    )
    assert_eq(
        pad["domain"],
        NODE_DOMAIN,
        "the index space is the NODE — one position, itself — which is what "
        "makes a uniformly painted thing attributable at all",
    )
    names = [r["name"] for r in pad["runs"]]
    assert_eq(
        names,
        [MARK_HUE_BASE, MARK_SV_THUMB],
        "both reasons, in declaration order, and they come from DIFFERENT "
        "externals — the composed fact no single oracle owns",
    )
    for run_ in pad["runs"]:
        assert_eq(
            (run_["start"], run_["end"]),
            (0, 1),
            "a node-domain run covers the one position there is; a [0, 0) run "
            "would publish a reason that answers no question",
        )
    print(f"[demo] {SV_TAG} publishes {names} over domain {pad['domain']!r}")

    # ---- 2. the stack at the one position, and its resolution ------------
    at = marks(tf, SV_TAG, 0)["at"]
    assert_eq(at["index"], 0)
    assert_eq(
        at["names"],
        [MARK_HUE_BASE, MARK_SV_THUMB],
        "the whole stack, innermost last — a person asking 'why' gets every "
        "reason rather than the one that happened to win",
    )
    assert_eq(
        at["top"],
        MARK_SV_THUMB,
        "and the LAST declared is the one a painter obeyed, which is the same "
        "direction positional marks resolve in — one rule, not two",
    )
    # There is no position 1: a uniform node is one place.
    assert_eq(marks(tf, SV_TAG, 1)["at"]["names"], [])
    assert_eq(marks(tf, SV_TAG, 1)["at"]["top"], None)

    # ---- 3. the channel and the publication answer DIFFERENT questions ----
    #        The pad is a container, so its channel is `structural`: the
    #        attribution of its CONTENT belongs to its children, and that is
    #        still true. Its own fill is a uniform paint, and that is what it
    #        publishes. A model where "structural" meant "nothing of its own"
    #        could not describe this node at all — and this node is the
    #        commonest shape there is, a panel with a background.
    assert_eq(
        pad["channel"],
        "structural",
        "where the CONTENT's attribution lives is a different question from "
        "why this node's own fill is that colour",
    )
    assert_eq(
        pad["published"],
        True,
        "...and it answers the second one",
    )

    # ---- 4. silent and no-channel stay different answers -----------------
    hue = marks(tf, HUE_TAG)
    assert_eq(
        hue["published"],
        False,
        "the hue bar was not asked to publish, so it is SILENT",
    )
    assert_eq(hue.get("domain"), None, "a silent node states no index space")
    assert hue["channel"] in ("uniform", "structural", "opaque", "carries"), (
        f"every node still declares its channel: {hue['channel']!r}"
    )

    # ---- 5. the attribution is DERIVED, not a label written once ---------
    before = float(tf.query(f"/{HUE_TAG}/external/value"))
    tf.intervene(f"/{HUE_TAG}/external/value", 0.75)
    for _ in range(2):
        tf.tick(0.016)
    after_hue = float(tf.query(f"/{HUE_TAG}/external/value"))
    assert after_hue != before, "the hue actually moved, so the check below bites"
    after_pad = marks(tf, SV_TAG)
    assert_eq(
        [r["name"] for r in after_pad["runs"]],
        [MARK_HUE_BASE, MARK_SV_THUMB],
        "the pad's reasons survive a state change — they are re-derived each "
        "frame from the same composition the paint used",
    )
    assert_eq(after_pad["domain"], NODE_DOMAIN)
    print(f"[demo] hue {before} -> {after_hue}; the pad still attributes its base to it")

    # ---- 6. an unknown tag is REFUSED, not answered ----------------------
    #        "there is no such node" and "that node publishes nothing" are
    #        different facts, and the first is an error rather than a quiet
    #        empty reply a client would read as the second.
    assert_rpc_error(
        lambda: call(tf, "scene/marks", {"tag": "no-such-node"}),
        data="UnknownTag: no-such-node",
    )

    # ---- 7. driving the OTHER external keeps both reasons ----------------
    #        Symmetry matters: if only the hue moved the publication, the pad
    #        would be attributing one external and mentioning the other.
    tf.intervene(f"/{SV_TAG}/external/x", 0.25)
    tf.intervene(f"/{SV_TAG}/external/y", 0.9)
    for _ in range(2):
        tf.tick(0.016)
    both = marks(tf, SV_TAG, 0)
    assert_eq(both["published"], True)
    assert_eq(both["domain"], NODE_DOMAIN)
    assert_eq(len(both["runs"]), 2, "still exactly two reasons")
    assert_eq(both["at"]["names"], [MARK_HUE_BASE, MARK_SV_THUMB])
    assert_eq(both["at"]["top"], MARK_SV_THUMB)
    assert_eq(
        float(tf.query(f"/{SV_TAG}/external/x")),
        0.25,
        "the pad's own external moved too, so the reasons above are not frozen",
    )

    # ---- 8. every run is a complete record -------------------------------
    for run_ in both["runs"]:
        assert set(run_) == {"name", "start", "end"}, (
            f"a run is exactly name + range: {run_!r}"
        )
        assert run_["name"], "a nameless run is the thing this axis replaces"
        assert run_["end"] > run_["start"], "an empty run answers no question"

    # ---- 9. the answer's own shape is published --------------------------
    schema = call(tf, "rpc/schema")
    outcome = next(
        (t for t in schema["types"] if t["name"] == "MarksOutcome"),
        None,
    )
    assert outcome is not None, "the reply shape is in the census"
    keys = {f["name"] for f in outcome["shape"]["fields"]}
    for expected in ("tag", "kind", "channel", "published", "domain", "runs"):
        assert expected in keys, (
            f"an agent reads the reply's shape before parsing it; {expected} is "
            f"missing from {sorted(keys)}"
        )

    print("[demo] a uniformly painted node names the reasons behind its colour")


def body() -> None:
    with RpcSubprocess("hello-color-picker", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("r1618 a node says why it is that colour", body)
