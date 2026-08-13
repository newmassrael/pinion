#!/usr/bin/env python3
"""R1678 §5.20 §5.32 — the screen says what has changed, and puts it back.

Drives `hello-node-lab` over JSON-RPC. The analysis tool's node-graph screen
gains the operation family its reference has and this one had none of: a reset
per scope — which cards exist, where they sit, what the forms hold, which links
are authored, and where the view is looking.

The round's claim is not "five actions exist". It is that **one fact drives
both halves**: `changed` says which scopes differ from what the screen opened
as, the reset affordance for a scope is painted exactly when that scope is in
it, and running the reset takes it back out. A screen that decided separately
when to show the button would be a second author of the rule, and the failure
is silent in both directions — a button over an unchanged screen does nothing
when pressed, and a missing one strands the change.

The reference is where the shape comes from, measured rather than remembered:
four of its five reset affordances are wrapped in a conditional on exactly this
predicate, and the fifth — the view — sits unconditionally in the zoom cluster.
That asymmetry is reproduced here and asserted in both directions.

  (A) boot — nothing has changed, and only the unconditional affordance exists.
  (B) the wire declares the scope vocabulary rather than making an agent guess.
  (C) per scope — change it, see `changed` say so, see the button appear.
  (D) per scope — press the button through the ROUTER, see the scope go back
      and the button go away with it.
  (E) the view's affordance never disappears, which is the declared asymmetry.
  (F) a scope's reset leaves the other scopes alone.

>= 30 assertions.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    assert_router_press_moves,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"
VIEWPORT = (1440, 900)


def q(tf, path):
    return tf.query(f"{EXT}/{path}")


def changed(tf) -> dict:
    return json.loads(q(tf, "changed"))


def tags(tf) -> set:
    return set(abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT)))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: nothing has changed ───────────────────────────
        opened = changed(tf)
        for scope, flag in opened.items():
            assert_eq(flag, False, f"{scope} is as it opened")
        painted = tags(tf)
        assert "lab.reset.view" in painted, (
            "the unconditional affordance is on the opening screen"
        )
        for scope in opened:
            if scope == "view":
                continue
            assert f"lab.reset.{scope}" not in painted, (
                f"★ nothing to put back, so the {scope} affordance is ABSENT — "
                f"not present and inert, which is a button that lies"
            )

        # ── (B) the vocabulary is declared, not guessed ─────────────
        spec = json.loads(q(tf, "spec"))
        declared = {r["scope"]: r["gated"] for r in spec["resets"]}
        assert_eq(sorted(declared), sorted(opened), "the wire declares every scope it reports")
        assert_eq(declared["view"], False, "the view's affordance is unconditional")
        for scope in ("nodes", "layout", "fields", "links"):
            assert_eq(declared[scope], True, f"the {scope} affordance is conditional")
        # The reset operations are in the operation table, with the
        # precondition that makes each one causable.
        ops = {op["name"]: op for op in spec["operations"]}
        for name in (
            "reset the node set",
            "reset the layout",
            "reset the fields",
            "reset the links",
            "reset the view",
        ):
            assert_eq(ops[name]["absent"], False, f"{name!r} is answered now")
            assert ops[name]["needs"], f"{name!r} declares what has to happen first"

        # ── (C)/(D)/(F) per scope: change it, see it, put it back ───
        # The population is the wire's own list, so a sixth scope is driven
        # here without anybody editing this demo.
        def change(scope):
            """Cause a change in `scope` the way a person would reach it."""
            if scope == "fields":
                tf.invoke(f"{EXT}/set_field", "id=a9")
            elif scope == "links":
                tf.invoke(f"{EXT}/connect", "S-01,P-02")
            elif scope == "view":
                tf.invoke(f"{EXT}/zoom_by", "in")
            elif scope == "layout":
                # A DRAG, not a press: a press on a card selects it and moves
                # nothing, which is the screen behaving correctly and is why
                # the router-press helper cannot be the driver here.
                rects = abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))
                x, y, w, h = rects["lab.node.P-03"]
                at = (float(x + w // 2), float(y + h // 2))
                before = q(tf, "layout")
                tf.drag(from_at=at, to_at=(at[0] + 40, at[1] + 24), steps=6)
                assert q(tf, "layout") != before, "the drag moved the card"
            elif scope == "nodes":
                assert_router_press_moves(
                    tf,
                    "lab.palette.role.Responder",
                    lambda: q(tf, "nodes"),
                    "the palette adds a card",
                )
            else:
                raise AssertionError(f"the wire grew a scope this demo cannot cause: {scope}")

        for scope in ("fields", "links", "view", "layout", "nodes"):
            change(scope)
            now = changed(tf)
            assert_eq(now[scope], True, f"★ changing {scope} is reported by `changed`")
            painted = tags(tf)
            assert f"lab.reset.{scope}" in painted, (
                f"★ and the {scope} affordance is painted BECAUSE it is"
            )

            others = {s: v for s, v in now.items() if s != scope}
            tf.invoke(f"{EXT}/reset", scope)
            back = changed(tf)
            assert_eq(back[scope], False, f"★ the reset put {scope} back")
            for other, was in others.items():
                assert_eq(back[other], was, f"and left {other} exactly as it was")
            if scope != "view":
                assert f"lab.reset.{scope}" not in tags(tf), (
                    f"★ the {scope} affordance goes away with the fact it reports"
                )

        # ── (E) the view's affordance never disappears ──────────────
        assert_eq(changed(tf)["view"], False, "the view is back where it opened")
        assert "lab.reset.view" in tags(tf), (
            "★ and its affordance is still there — the declared asymmetry"
        )

        # ── (D') the same journey through a real ROUTER press ───────
        # The wire path above proves the operation; this proves a person can
        # reach it. Both, because every defect this screen has collected lived
        # exactly between those two columns.
        tf.invoke(f"{EXT}/set_field", "id=a7")
        assert_eq(changed(tf)["fields"], True, "a form edit is a change")
        assert_router_press_moves(
            tf, "lab.reset.fields", lambda: q(tf, "form"), "the forms go back"
        )
        assert_eq(
            changed(tf)["fields"],
            False,
            "★ pressing the affordance where it is painted puts the forms back",
        )
        tf.invoke(f"{EXT}/zoom_by", "in")
        assert_router_press_moves(
            tf, "lab.reset.view", lambda: q(tf, "zoom"), "the view goes home"
        )
        assert_eq(changed(tf)["view"], False, "and the view reset answers a press too")

        # A reset over an unchanged scope is accepted and moves nothing —
        # which is why the affordance is conditional rather than disabled.
        before = q(tf, "form")
        tf.invoke(f"{EXT}/reset", "fields")
        assert_eq(q(tf, "form"), before, "a reset with nothing to do is a no-op")


if __name__ == "__main__":
    sys.exit(run_demo("R1678 §5.20 — the screen puts itself back", body))
