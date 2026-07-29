#!/usr/bin/env python3
"""R1483 §5.34 §5.45 §2 #2 — one name reaches one surface, both compositions.

`CoreShell::compose_root` gives a binding's primary External the
`WidgetCore::tag()`, then places it at the **root** when the binding has no
extra externals, or inside a `Container` when it has some. Path segments come
from a node's parent, so a root's tag was never an address. Measured before
the fix, on the same tag naming the same logical surface:

    extras = 0  ->  /model/external/count = NoExternalAtPath
    extras = 1  ->  /model/external/count = Ok(1)

So whether a binding's primary answered to its own name depended on a
composition detail no client can see. Worse, R688 made the external set a
reactive projection of state — `reconcile_externals` re-composes at runtime,
so a working address could stop working because an unrelated extra surface
appeared or went away. That is §2 #2's stable-address contract failing.

This demo drives one binding of each composition and asserts the tag reaches
the primary in both, on the read AND write channels:

    hello-answer-origin   no extras, primary at the root       tag "model"
    hello-listbox         one extra, primary inside a wrap     tag "main_list"

The second is the control: its tag address worked before this round, so it
proves the fix made the two shapes AGREE rather than merely changing one.

ZERO-FLAKE: every assertion is a value this demo wrote, a declared schema
path, or a typed refusal. Nothing waits on wall-clock or pixels.

Run from the workspace root:
    cargo build -p hello-answer-origin -p hello-listbox --release
    python3 tools/demos/r1483_stable_tag_address.py
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

# (example, primary tag, a readable slot on the primary, composition)
BARE_ROOT = ("hello-answer-origin", "model", "ticks", "no extras — primary IS the root")
WRAPPED = ("hello-listbox", "main_list", "count", "one extra — primary is a child")


def assert_tag_reaches_primary(tf: RpcSubprocess, tag: str, slot: str, what: str) -> None:
    """The tag address and the bare shorthand must name the same surface."""
    by_tag = tf.query(f"/{tag}/external/{slot}")
    by_shorthand = tf.query(f"/external/{slot}")
    assert by_tag is not None, f"{what}: /{tag}/external/{slot} answered nothing"
    assert_eq(by_tag, by_shorthand, f"★ {what}: the tag names the same surface as /external")


def body() -> None:
    # ── (A) the shape with no extras: the address that did not exist ────────
    example, tag, slot, what = BARE_ROOT
    with RpcSubprocess(example, boot_grace=1.5) as tf:
        # Premise: this really is the bare-root composition. `scene/snapshot
        # from=state` reports the root node's own type, so the demo verifies
        # the shape it claims to be testing rather than assuming it.
        state = tf.snapshot(source="state")
        assert isinstance(state, dict), f"A: state snapshot is a node, got {type(state)}"
        assert_eq(state.get("type"), "External", "★A: the primary IS the state-scene root")
        assert_eq(state.get("tag"), tag, "★A: and the root carries the binding's tag")

        assert_tag_reaches_primary(tf, tag, slot, what)

        # The write channel must resolve the same address, or the wire says a
        # path both exists and does not (the R1481 class).
        tf.intervene(f"/{tag}/external/{slot}", 41)
        assert_eq(
            tf.query(f"/{tag}/external/{slot}"), 41, f"★A: the write reached {tag} by name"
        )
        assert_eq(
            tf.query(f"/external/{slot}"), 41, "★A: …and the shorthand sees the same surface"
        )
        tf.intervene(f"/{tag}/external/{slot}", 5)
        assert_eq(tf.query(f"/external/{slot}"), 5, "★A: not a one-off")
        # …and the other direction: one surface, so a write through the
        # shorthand is seen by name. Two aliases for one node, not two nodes.
        tf.intervene(f"/external/{slot}", 9)
        assert_eq(
            tf.query(f"/{tag}/external/{slot}"), 9, "★A: the two addresses are one surface"
        )
        assert_eq(
            tf.query(f"/{tag}/external/{slot}", with_origin=True).get("origin"),
            tf.query(f"/external/{slot}", with_origin=True).get("origin"),
            "★A: and they disclose the same origin",
        )

        # Discovery agrees too: a client that reads the contract by tag must
        # get the contract of the surface the tag addresses.
        by_tag = tf.query(f"/{tag}/external/$schema")
        by_shorthand = tf.query("/external/$schema")
        assert isinstance(by_tag, list) and by_tag, f"A: schema by tag reads {by_tag!r}"
        assert_eq(by_tag, by_shorthand, "★A: one contract, addressed two ways")

        # Every declared slot answers through the tag address, walked from the
        # surface's own schema so adding a slot cannot make this test less.
        declared = [f["path"] for f in by_tag]
        assert declared, "A: the primary declares at least one slot"
        for path in declared:
            assert tf.query(f"/{tag}/external/{path}") is not None, (
                f"★A: declared slot {path} must answer through the tag address"
            )

        # The alias names ONE node: the root's tag must not become a silent
        # path prefix that swallows whatever follows it.
        assert_rpc_error(
            lambda: tf.query(f"/{tag}/deeper/external/{slot}"), data="NoExternalAtPath"
        )
        # A name that is nobody's is still refused — the alias did not turn
        # resolution into a wildcard.
        assert_rpc_error(lambda: tf.query(f"/nosuchtag/external/{slot}"), data="NoExternalAtPath")
        assert_rpc_error(
            lambda: tf.intervene(f"/nosuchtag/external/{slot}", 1), data="NoExternalAtPath"
        )

        # R1482 parity: the alias changes which paths resolve, not what an
        # answer IS. A state-scene surface reached by name is still `state`.
        disclosed = tf.query(f"/{tag}/external/{slot}", with_origin=True)
        assert isinstance(disclosed, dict), f"A: disclosing read returns an object, got {disclosed!r}"
        assert_eq(disclosed.get("origin"), "state", "★A: reached by name, still the state scene")
        assert_eq(disclosed.get("value"), 9, "A: and the value the write placed")

    # ── (B) the shape with extras: the control that already worked ──────────
    example, tag, slot, what = WRAPPED
    with RpcSubprocess(example, boot_grace=1.5) as tf:
        # Premise: this binding really is the OTHER composition. If it were
        # also a bare root, (B) would be a second copy of (A) rather than the
        # cross-composition claim the round is about.
        state = tf.snapshot(source="state")
        assert isinstance(state, dict), f"B: state snapshot is a node, got {type(state)}"
        assert_eq(state.get("type"), "Container", "★B: the primary is inside a wrap here")
        children = state.get("children") or []
        assert len(children) >= 2, f"B: the wrap holds the primary plus an extra, got {len(children)}"
        assert_eq(children[0].get("tag"), tag, "★B: the primary is the wrap's head")

        assert_tag_reaches_primary(tf, tag, slot, what)

        # The extra sibling is reachable by ITS tag — the case where a tag is
        # the only address, and the behaviour the bare-root primary now shares.
        extra_tag = children[1].get("tag")
        assert extra_tag and extra_tag != tag, f"B: the extra has its own tag, got {extra_tag!r}"
        extra_schema = tf.query(f"/{extra_tag}/external/$schema")
        assert isinstance(extra_schema, list) and extra_schema, (
            f"★B: the extra answers by tag: {extra_schema!r}"
        )

        # …and the primary's own contract is addressable by name here too, so
        # both compositions expose the same three addresses.
        by_tag = tf.query(f"/{tag}/external/$schema")
        assert isinstance(by_tag, list) and by_tag, f"B: schema by tag reads {by_tag!r}"
        assert_eq(by_tag, tf.query("/external/$schema"), "★B: one contract, addressed two ways")
        # The tag SELECTS rather than resolving to whatever is nearest: the
        # extra's contract must differ from the primary's, or every assertion
        # here would pass for a walker that ignored the name.
        assert extra_schema != by_tag, (
            "★B: the extra and the primary are different surfaces, so the tag chose"
        )

        assert_rpc_error(lambda: tf.query(f"/nosuchtag/external/{slot}"), data="NoExternalAtPath")
        assert_eq(
            tf.query(f"/{tag}/external/{slot}", with_origin=True).get("origin"),
            "state",
            "★B: the wrapped primary answers from the state scene too",
        )


if __name__ == "__main__":
    run_demo("R1483 stable tag address", body)
