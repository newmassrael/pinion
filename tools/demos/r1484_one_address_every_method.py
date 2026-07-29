#!/usr/bin/env python3
"""R1484 §5.32 §5.34 §2 #2 — one address, honoured by every method.

R1483 gave the external-path methods a root-name alias so a binding's primary
External answers to its own tag in both compositions. It left the other
client-path consumers on the bare walk, and that split was measurable on this
very binding:

    scene/query  /model/external/ticks  ->  0
    scene/bbox   /model                 ->  UnknownPath

The same name, the same node, two verdicts — an inconsistency the previous
round introduced by fixing one layer. An address a client learns from one
method has to work in the others, or "stable address" (§2 #2) means only
"stable within whichever method you happened to try".

R1484 routes every RPC site that resolves a client-supplied scene path through
one rule: `scene/bbox`, and the `SetStyle` / `ReplaceView` preview targets,
join the five external-path sites R1483 already covered. An in-crate source
scan (`r1484_every_rpc_walk_of_a_client_path_goes_through_the_shared_rule`)
keeps a new handler from quietly reintroducing the split.

ZERO-FLAKE: every assertion is a name this demo asked about, a value it wrote,
or a typed refusal. Nothing waits on wall-clock or pixels.

Run from the workspace root:
    cargo build -p hello-answer-origin --release
    python3 tools/demos/r1484_one_address_every_method.py
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
)

EXAMPLE = "hello-answer-origin"
NAME = "model"
SLOT = "ticks"


def bbox(tf: RpcSubprocess, path: str):
    resp = tf.request("scene/bbox", {"path": path})
    assert resp is not None
    return resp.result


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) premise: the root really is the named node ──────────────────
        state = tf.snapshot(source="state")
        assert isinstance(state, dict), f"A: state snapshot is a node, got {type(state)}"
        assert_eq(state.get("type"), "External", "★A: the primary IS the state-scene root")
        assert_eq(state.get("tag"), NAME, "★A: and it carries the binding's tag")

        # ── (B) the read that already worked after R1483 ────────────────────
        by_name = tf.query(f"/{NAME}/external/{SLOT}")
        assert by_name is not None, "B: the read answers the name"
        assert_eq(by_name, tf.query(f"/external/{SLOT}"), "★B: one surface, two addresses")

        # ── (C) the method that used to disagree ────────────────────────────
        # Measured before this round: `UnknownPath`. The claim is agreement,
        # not a particular rect — the root's rect is whatever it is, and both
        # addresses must report the SAME one.
        named = bbox(tf, f"/{NAME}")
        assert isinstance(named, dict) and "bbox" in named, f"★C: /{NAME} has a bbox: {named!r}"
        assert_eq(named, bbox(tf, "/"), "★C: the name and the empty path are one node")

        # ── (D) …and the mutating path consumer, end to end ─────────────────
        # `scene/propose_change` only RECORDS a proposal; the walk happens in
        # `scene/apply_preview`. Asserting on the proposal alone would pass
        # with the fix reverted, which a counterfactual demonstrated.
        #
        # This binding's state root is an `External`, which carries no
        # BoxStyle, so `SetStyle` is refused here either way — and that makes
        # a SHARPER witness than a success would: the two refusals say
        # different things. `UnknownTarget` means the walk never found the
        # node (the pre-R1484 answer); `UnsupportedStyleTarget` means it
        # reached the node and the node's type has no fill. Telling those
        # apart is the §2 #7 contract, and only the second is honest here.
        def apply_set_style(target: str):
            proposal = tf.request(
                "scene/propose_change",
                {"kind": "SetStyle", "target_path": target, "style": {"fill": 16711935}},
            )
            assert proposal is not None and proposal.result, f"D: proposal for {target}"
            pid = proposal.result.get("preview_id")
            assert pid is not None, f"D: a preview id came back: {proposal.result!r}"
            return lambda: tf.request("scene/apply_preview", {"preview_id": pid})

        assert_rpc_error(
            apply_set_style(f"/{NAME}"),
            data={"reason": "UnsupportedStyleTarget", "variant": "ApplyRejected"},
        )

        # ── (E) the surface still answers, and is still one node ────────────
        assert_eq(
            tf.query(f"/{NAME}/external/{SLOT}"),
            tf.query(f"/external/{SLOT}"),
            "★E: still one surface under both addresses",
        )
        assert_eq(
            tf.query(f"/{NAME}/external/{SLOT}", with_origin=True).get("origin"),
            "state",
            "★E: and still the state scene answering (R1482)",
        )

        # ── (F) the write channel agrees with the readers ───────────────────
        tf.intervene(f"/{NAME}/external/{SLOT}", 33)
        assert_eq(tf.query(f"/{NAME}/external/{SLOT}"), 33, "★F: the write reached it by name")
        assert_eq(tf.query(f"/external/{SLOT}"), 33, "★F: seen through the other address too")

        # ── (G) no method became a wildcard ─────────────────────────────────
        # The alias may only turn a former refusal into an answer for a name
        # that IS the root's; a name nobody has must still be refused, by
        # every one of them.
        assert_rpc_error(lambda: tf.query(f"/ghost/external/{SLOT}"), data="NoExternalAtPath")
        assert_rpc_error(lambda: tf.intervene(f"/ghost/external/{SLOT}", 1), data="NoExternalAtPath")
        assert_rpc_error(lambda: bbox(tf, "/ghost"), data="UnknownPath")
        # …and the contrast that gives (D) its meaning: a target nobody has is
        # refused for a DIFFERENT reason — not found, rather than found and
        # unsuitable. One rule, two honest answers.
        assert_rpc_error(
            apply_set_style("/ghost"),
            data={"reason": "UnknownTarget", "variant": "ApplyRejected"},
        )

        # ── (H) the alias names ONE node, in every method ───────────────────
        # The root's tag must not become a silent prefix that swallows the
        # rest of the path — checked on each consumer, not just the reader.
        assert_rpc_error(
            lambda: tf.query(f"/{NAME}/deeper/external/{SLOT}"), data="NoExternalAtPath"
        )
        assert_rpc_error(lambda: bbox(tf, f"/{NAME}/deeper"), data="UnknownPath")

        # ── (I) the whole declared surface is reachable by the name ─────────
        # Walked from the surface's own schema, so a new slot cannot make this
        # test cover less than the surface it claims to cover.
        schema = tf.query(f"/{NAME}/external/$schema")
        assert isinstance(schema, list) and schema, f"I: schema reads {schema!r}"
        declared = [f["path"] for f in schema]
        for path in declared:
            assert tf.query(f"/{NAME}/external/{path}") is not None, (
                f"★I: declared slot {path} must answer through the name"
            )
        assert_eq(
            schema, tf.query("/external/$schema"), "★I: one contract under both addresses"
        )

        # ── (J) the OTHER preview target walks the same rule ───────────────
        # LAST, because it succeeds: replacing the root removes the External
        # every section above addresses, so this must not run before them.
        # `SetStyle` and `ReplaceView` are separate walk sites; covering only
        # one would leave the other free to keep the old behaviour. This one
        # is asserted through the same reached-vs-not-found contrast.
        def apply_replace(target: str):
            proposal = tf.request(
                "scene/propose_change",
                {
                    "kind": "ReplaceView",
                    "target_path": target,
                    "replacement": {
                        "kind": "Box",
                        "rect": {"x": 0, "y": 0, "w": 4, "h": 4},
                        "style": {"fill": 255},
                        "tag": "replaced",
                    },
                },
            )
            assert proposal is not None and proposal.result, f"J: proposal for {target}"
            pid = proposal.result.get("preview_id")
            assert pid is not None, f"J: a preview id came back: {proposal.result!r}"
            return tf.request("scene/apply_preview", {"preview_id": pid})

        # A name nobody has is not found…
        assert_rpc_error(
            lambda: apply_replace("/ghost"),
            data={"reason": "UnknownTarget", "variant": "ApplyRejected"},
        )
        # …while the root's own name IS found, and the replacement lands.
        replaced = apply_replace(f"/{NAME}")
        assert replaced is not None and replaced.result, "★J: ReplaceView reached the named root"
        assert replaced.result.get("new_revision") is not None, (
            f"★J: and the apply bumped the revision: {replaced.result!r}"
        )
        # The scene now holds the replacement under that address — the walk
        # reached the node the name meant, not some neighbour.
        assert_eq(
            bbox(tf, "/").get("bbox"),
            {"x": 0, "y": 0, "w": 4, "h": 4},
            "★J: the root IS the replacement the proposal named",
        )
        assert_rpc_error(lambda: tf.query(f"/{NAME}/external/{SLOT}"), data="NoExternalAtPath")



if __name__ == "__main__":
    sys.exit(run_demo("R1484 one address, every method", body))
