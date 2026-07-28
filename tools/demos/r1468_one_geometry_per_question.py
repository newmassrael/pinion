#!/usr/bin/env python3
"""R1468 §5.23 §5.22 §2 #3 §2 #7 — a hypothetical-viewport question is answered
in ONE geometry, and answering it changes nothing.

`scene/layout {viewport}` asks a binding to lay itself out as if the window were
W x H. `hello-viewport-question` draws the same quantity — half the window's
height — twice, by the two routes a binding has:

  - `by_engine` is `height: 50%`, resolved by the layout engine against whatever
    root extent it is handed;
  - `by_seam` is `use_viewport_size().1 / 2`, read through the R1006 reactive
    seam a producer needs because a reflow is a SIDE EFFECT (a PTY's winsize
    ioctl) and so must be reachable from an `Effect`, not only from the view.

Pre-R1468 those two answered a hypothetical differently: taffy got the
hypothetical extent, the seam kept reporting the live one. One question, two
geometries — and since R1467 the window chrome and its content inset sat on the
hypothetical side of the split, so an agent sizing a window was told about a
window that will never exist.

Publishing the hypothetical is only half a fix, because a publish is a
`Signal::set` and a `Signal::set` re-runs subscribers. This binding exposes
`reflows` — how many times the seam-subscribed `Effect` actually ran — on the
§5.15 introspect channel, so BOTH halves are checked over the wire:

  (B) the two routes agree at a hypothetical extent;
  (C) the live extent is unchanged afterwards;
  (D) `reflows` did not move, however many questions were asked;
  (E) …while a REAL resize still reflows, which is what proves (D) is
      containment and not a broken seam.

ZERO-FLAKE: every assertion compares values produced by the same run — two
nodes of one answer, or one counter either side of a call — and nothing waits on
wall-clock or on pixels. The only waits are `wait_until` on the app's first
paint and on the resize the compositor must acknowledge, both of which are the
assertions themselves.

Run from the workspace root:
    cargo build -p hello-viewport-question --release
    python3 tools/demos/r1468_one_geometry_per_question.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-viewport-question"
WIN_W, WIN_H = 640, 420
BY_ENGINE = "by_engine"
BY_SEAM = "by_seam"
REFLOW_TAG = "reflow_meter"
# Deliberately not multiples of WIN_H, so a bar measured against the wrong
# extent cannot coincide with one measured against the right one. All EVEN,
# because the two routes round differently on a half pixel — taffy resolves
# `50% of 901` to 451 and Rust's `901 / 2` is 450. That ±1 is a rounding step,
# not the two-geometry split this demo is about, and mixing the two would make
# a green run mean less rather than more.
HYPOTHETICALS = (1200, 260, 900)


def walk(node: Any):
    """Yield every node of a `scene/layout` tree."""
    if not isinstance(node, dict):
        return
    yield node
    for child in node.get("children") or []:
        yield from walk(child)


def height_of(tree: Any, tag: str) -> Optional[int]:
    for node in walk(tree):
        if node.get("tag") == tag:
            return int(node["rect"]["h"])
    return None


def layout_at(tf: RpcSubprocess, height: int) -> Any:
    """The PRODUCER's tree: `scene/layout` with an explicit viewport runs the
    produce closure at that size (`viewport: None` reads the stored frame)."""
    resp = tf.request("scene/layout", {"viewport": {"width": WIN_W, "height": height}})
    assert resp is not None
    return resp.result


def painted(tf: RpcSubprocess) -> Any:
    """The STORED frame's layout — what the window actually shows."""
    resp = tf.request("scene/layout", {})
    assert resp is not None
    return resp.result


def reflows(tf: RpcSubprocess) -> int:
    return int(tf.query(f"{REFLOW_TAG}/external/reflows"))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) premise: the two routes already agree at the live extent ────
        def both_bars() -> Any:
            tree = painted(tf)
            return tree if height_of(tree, BY_SEAM) not in (None, 1) else None

        live = wait_until(both_bars, desc="the window paints and both bars measure")
        live_engine = height_of(live, BY_ENGINE)
        live_seam = height_of(live, BY_SEAM)
        assert live_engine is not None, "A: the engine-sized bar is in the tree"
        assert live_seam is not None, "A: the seam-sized bar is in the tree"
        assert_eq(
            live_seam,
            live_engine,
            "A: THE PREMISE — at the live extent the two routes agree, so a "
            "disagreement below is about the QUESTION and not about the binding",
        )
        assert live_engine > 1, f"A: the live bars have a real height: {live_engine}"

        # ── (B) one question, one geometry ──────────────────────────────────
        for h in HYPOTHETICALS:
            tree = layout_at(tf, h)
            engine = height_of(tree, BY_ENGINE)
            seam = height_of(tree, BY_SEAM)
            assert engine is not None, f"B: the engine bar answers at {h}"
            assert seam is not None, f"B: the seam bar answers at {h}"
            assert_eq(
                engine,
                h // 2,
                f"B: premise — the layout engine answers in the hypothetical at {h}",
            )
            assert_eq(
                seam,
                engine,
                f"★B: BOTH routes describe the same window at {h} — pre-R1468 "
                f"the seam answered {live_seam} here, the LIVE half",
            )
            assert_eq(
                int(tree["rect"]["h"]),
                h,
                f"B: and the root itself is the size asked about ({h})",
            )

        # ── (C) the world is where it was ───────────────────────────────────
        after = painted(tf)
        assert_eq(
            height_of(after, BY_SEAM),
            live_seam,
            "★C: the seam is back at the live extent — a question that left the "
            "hypothetical behind would make the next real paint a CHANGE",
        )
        assert_eq(
            height_of(after, BY_ENGINE),
            live_engine,
            "C: …and so is the engine-sized bar",
        )

        # ── (D) …and nothing reflowed ───────────────────────────────────────
        # The half that makes publishing safe. A `Signal::set` re-runs
        # subscribers, so republishing the hypothetical without a containment
        # scope would drive this counter at a size no window has.
        before_reflows = reflows(tf)
        for h in HYPOTHETICALS:
            _ = layout_at(tf, h)
            assert_eq(
                reflows(tf),
                before_reflows,
                f"★D: asking about {h} fired no reflow — the side effect the "
                f"seam exists for stays bound to REAL sizes",
            )
        # Reads that are not questions must be free too (the control that keeps
        # (D) about containment rather than about a dead counter).
        _ = painted(tf)
        _ = tf.snapshot(source="paint")
        assert_eq(
            reflows(tf),
            before_reflows,
            "D: plain reads reflow nothing either",
        )

        # ── (E) a REAL resize still reflows ─────────────────────────────────
        # Without this, (D) would be satisfied by a seam that never fires at
        # all — the failure mode that looks identical from the outside.
        new_h = WIN_H + 120
        resp = tf.request("scene/resize", {"width": WIN_W, "height": new_h})
        assert resp is not None and resp.result is not None, "E: scene/resize accepted"

        def resized() -> Optional[int]:
            n = reflows(tf)
            return n if n > before_reflows else None

        after_reflows = wait_until(resized, desc="a real resize reaches the seam")
        assert after_reflows > before_reflows, (
            f"★E: a REAL resize reflows ({before_reflows} -> {after_reflows}) — "
            f"containment is scoped to the question, not a disabled seam"
        )
        resized_tree = painted(tf)
        assert_eq(
            height_of(resized_tree, BY_SEAM),
            height_of(resized_tree, BY_ENGINE),
            "★E: and the two routes still agree at the new live extent",
        )
        assert height_of(resized_tree, BY_SEAM) != live_seam, (
            "E: the window really is a different size now"
        )

        # ── (F) the questions are still answerable, and still contained ─────
        settled = reflows(tf)
        for h in HYPOTHETICALS:
            tree = layout_at(tf, h)
            assert_eq(
                height_of(tree, BY_SEAM),
                height_of(tree, BY_ENGINE),
                f"★F: one geometry at {h}, from the resized window too",
            )
            assert_eq(
                reflows(tf),
                settled,
                f"F: and still no reflow at {h}",
            )
        assert_eq(
            height_of(painted(tf), BY_SEAM),
            height_of(resized_tree, BY_SEAM),
            "★F: the resized live extent survives the second round of questions",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1468 one geometry per question", body))
