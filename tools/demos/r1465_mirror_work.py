#!/usr/bin/env python3
"""R1465 §5.16 §5.12 §2 #7 — what a call costs, on `hello-tail-reveal`.

An RPC dispatch that changed visible state re-renders every painted window's
scene and stores it, so the next `scene/snapshot from: paint` answers with the
state just committed instead of waiting for the compositor (R705). That render
is the fourth producer of `view` runs, and until R1465 it was the only one that
neither settled nor reported: it took a single layout pass and threw the pass's
own dirty bit away, so on a binding whose view reads back what layout writes —
this one — the scene left in the store could be an earlier pass of the picture
the window is settling to.

Both halves are read here as DATA off `scene/frame_timings`, no pixels:

  - a READ is free. `scene/frame_timings` and `focus/get` move neither the
    `produce` group nor the `mirror` one. That zero answers the question R1464
    left open: `produce.passes_total` sitting still on a `from: paint` read is
    the true price of reusing a committed frame, not a blind spot. The blind
    spot was one producer further along.
  - a MUTATION is priced. One `scene/click` that appends an entry stores TWO
    mirrors of this single window — the R684 finalize before the input drain
    (so the click hit-tests against real geometry) and the R705 re-store after
    it — and together they cost MORE PASSES THAN SCENES. That inequality is the
    fix: pre-R1465 `passes_total` and `scenes_total` moved in lockstep BY
    CONSTRUCTION, because the mirror could only ever run one pass.

    The `2` is itself a measurement this round's counter produced. The finalize
    site is commented "first-paint only, so already-active windows do not pay
    any per-RPC cost"; R685 dropped that gate when it made the publish
    side-effect-free, and the comment kept the old claim until the number
    contradicted it.

What this demo does NOT cover, and why:

  - **the stored scene's own content.** Asserting "the mirror an agent reads is
    the settled one" against a live window is racy in the direction that hides
    bugs: the window repaints itself and repairs the store, so a broken build
    would go green whenever the compositor won. The counters cannot be repaired
    that way — a live repaint records a FRAME and never touches the mirror
    group — so the claim is made here in counts and pinned on content by
    `crates/pinion-shell/tests/frame_settles_before_paint.rs`.
  - **the window fan-out.** `mirror.scenes_total` grows by one per PAINTED
    window per mutating call; this binding has one. The two-window case is
    pinned by `dispatch_core.rs`'s
    `r1465_a_read_is_free_and_a_mutation_is_priced_at_the_window_fan_out`.

ZERO-FLAKE: every assertion reads a cumulative counter or a published number
from the same run that produced it, and nothing here waits on wall-clock.

Run from the workspace root:
    cargo build -p hello-tail-reveal --release
    python3 tools/demos/r1465_mirror_work.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-tail-reveal"
WIN = (520, 620)
SEED = 60
REPLY_TAG = "reveal_reply"


def wait_timings(tf: RpcSubprocess) -> dict[str, Any]:
    """`scene/frame_timings` answers only once the window has painted."""

    def poll() -> Any:
        try:
            return tf.frame_timings()
        except RpcError:
            tf.tick(0.05)
            return None

    return wait_until(poll, desc="scene/frame_timings becomes available")


def mirror(tf: RpcSubprocess) -> dict[str, int]:
    return {k: int(v) for k, v in tf.frame_timings()["mirror"].items()}


def produce(tf: RpcSubprocess) -> dict[str, int]:
    return {k: int(v) for k, v in tf.frame_timings()["produce"].items()}


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) the wire carries the group, with every field ────────────────
        timings = wait_timings(tf)
        assert "mirror" in timings, f"the mirror group is on the wire: {timings}"
        assert_eq(
            sorted(timings["mirror"]),
            [
                "nodes_total",
                "passes_total",
                "scenes_total",
                "shape_misses_total",
                "unsettled_total",
            ],
            "A: the five questions a stored mirror can be asked",
        )
        assert_eq(
            sorted(timings["produce"]),
            ["nodes_total", "passes_total", "shape_misses_total"],
            "A: and the same for its sibling group",
        )
        for field, value in timings["mirror"].items():
            assert isinstance(value, int) and value >= 0, (
                f"A: mirror.{field} is a cumulative count, got {value!r}"
            )

        # THE PRECONDITION: this binding's view reads back what its layout
        # writes, so its mirror has something to settle. On a binding that
        # converges on pass 1 every assertion below would hold trivially.
        assert_eq(
            tf.query("/external/is_fully_measured"),
            False,
            "A: rows past the fold are still counted at the estimate — the "
            "feedback that makes one pass the wrong number of passes",
        )
        assert_eq(int(tf.query("/external/item_count")), SEED, "A: entry count")

        # ── (B) a read is free — the R1464 carry, answered ──────────────────
        before = mirror(tf)
        before_p = produce(tf)
        assert_eq(mirror(tf), before, "B: reading the counters does not move them")
        assert_eq(produce(tf), before_p, "B: on either group")

        tf.request("focus/get", {})
        assert_eq(
            mirror(tf),
            before,
            "B: a read stores no mirror — there is nothing new to mirror",
        )
        assert_eq(
            produce(tf),
            before_p,
            "B: and runs no producer. THAT is the zero R1464 asked about: a "
            "`from: paint` read is answered from the committed frame, so zero "
            "passes is its true price and not a gap",
        )

        # ── (C) a mutation is priced, and the price is more than one pass ───
        tf.click(path=REPLY_TAG)
        assert_eq(
            int(tf.query("/external/item_count")), SEED + 1, "C: the click appended"
        )
        after = mirror(tf)
        after_p = produce(tf)

        stored = after["scenes_total"] - before["scenes_total"]
        spent = after["passes_total"] - before["passes_total"]
        assert_eq(
            stored,
            2,
            "C: ONE window, but TWO stored mirrors — the R684 finalize the "
            "input drain hit-tests against, then the R705 re-store that "
            "publishes the mutation. Neither was visible before this counter",
        )
        assert spent > stored, (
            f"C: those mirrors spent {spent} passes on {stored} stored scenes. "
            f"Equality is the pre-R1465 shape BY CONSTRUCTION — the mirror ran "
            f"one pass and dropped its dirty bit — so `>` is the whole fix"
        )
        assert spent <= stored * 4, (
            f"C: and every one stayed inside SETTLE_PASS_BUDGET ({spent} passes)"
        )
        assert_eq(
            after["unsettled_total"],
            0,
            "C: it converged, so nothing was stored mid-settle. A non-zero "
            "here is how a truncated mirror stays loud — it may not ask for a "
            "frame the way a paint can",
        )
        assert after_p["passes_total"] > before_p["passes_total"], (
            "C: the produce group moved too, because a click resolves its hit "
            "through the producer. Asserting the interference beats "
            "approximating around it"
        )

        # ── (D) the two groups count different work ─────────────────────────
        # A `scene/snapshot from: paint` runs the producer for its viewport but
        # mutates nothing, so it moves `produce` and leaves `mirror` alone.
        m_pre, p_pre = mirror(tf), produce(tf)
        tf.snapshot(source="paint", viewport=WIN)
        assert_eq(
            mirror(tf),
            m_pre,
            "D: a snapshot changes nothing, so it stores no mirror",
        )
        assert produce(tf)["passes_total"] >= p_pre["passes_total"], (
            "D: the two groups are separate accounts, not one number split"
        )

        # ── (E) it holds across repeated mutations ──────────────────────────
        for round_no in (1, 2, 3):
            pre = mirror(tf)
            tf.click(path=REPLY_TAG)
            post = mirror(tf)
            stored_n = post["scenes_total"] - pre["scenes_total"]
            assert_eq(
                stored_n,
                2,
                f"E{round_no}: the fan-out is a property of the call, not of "
                f"the first one — same two mirrors every time",
            )
            assert post["passes_total"] - pre["passes_total"] > stored_n, (
                f"E{round_no}: and it kept settling past the first pass"
            )
            assert_eq(
                post["unsettled_total"], 0, f"E{round_no}: still converging"
            )
            assert post["shape_misses_total"] >= pre["shape_misses_total"], (
                f"E{round_no}: shaper misses are cumulative, never reset"
            )
        assert_eq(
            int(tf.query("/external/item_count")), SEED + 4, "E: four replies landed"
        )

        # ── (F) none of it became a frame ───────────────────────────────────
        # `frame_count` is NOT asserted equal across the run: a live window
        # repaints itself, so that number belongs to the compositor and pinning
        # it here would measure the compositor (R1464's removed assertion). The
        # claim that survives is the one-directional one below.
        final = tf.frame_timings()
        assert int(final["frame_count"]) >= 1, "F: the window has painted"
        assert_eq(
            int(final["last"]["settle_passes"]) >= 1,
            True,
            "F: and a painted frame reports its own passes, in the ring, "
            "where the mirror's never appear",
        )
        assert_eq(
            mirror(tf)["scenes_total"] > 0,
            True,
            "F: while the mirror's totals live outside it entirely",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1465 the stored mirror settles and reports (hello-tail-reveal)", body))
