#!/usr/bin/env python3
"""R1538 §5.16 §5.27 §2 #2 — the frame states how much of the scene it walked.

The pro-tool-performance axis is named for a claim nothing in this tree could
check: *60fps with large scenes*. Every number the axis holds is a component
measured in isolation — a cache hit rate, an encode span, a GPU span — and the
obvious end-to-end check, timing a big binding and asserting a threshold,
cannot be a CI guard at all. A wall-clock assertion reads the host, so it
either flakes or is set so loose it proves nothing.

The way out is to notice what the claim actually is. "60fps at scale" is not a
statement about a clock, it is a **complexity** statement: per-frame work is
bounded by what is *visible*, not by how big the model is. That is
machine-independent, and R1538 made it readable — `scene/frame_timings` now
carries the frame's node census:

  * `last.scene_nodes`  — nodes in the tree the frame painted
  * `last.layout_nodes` — nodes measured across every settle pass
  * `last.encode_nodes` — nodes the encode walk entered (a fragment-cache hit
                          short-circuits its subtree, so this is far below
                          `scene_nodes` on a steady frame)
  * `last.access_nodes` — nodes the accessibility walk produced. A SECOND
                          traversal: `V::access_node` builds its own tree every
                          paint, so a binding can window its paint perfectly
                          and still enumerate its whole model to assistive
                          technology, doing O(model) work while every other
                          count stays flat.

with `window.max_*` peers, because the guarded property is an upper bound and
a mean cannot state one.

This demo asserts:

  (A) The census exists on the wire, is typed, and is internally coherent —
      `layout_nodes >= scene_nodes` (the settle loop cannot measure fewer
      nodes than the tree it produced), `encode_nodes <= scene_nodes` on a
      painted frame, and the `window.max_*` peers bound their `last`.

  (B) **Scale invariance.** Growing the dataset by a factor of 10,000 does not
      move `scene_nodes`. This is the end-to-end 60fps-at-scale claim, stated
      as a count, on a real binding, in one process — same window, same fonts,
      same caches, nothing varying but the model.

  (C) **The guard can fail.** The same binding has an eager arm that builds
      one node per row, and there the census MUST grow with the dataset. A
      scale guard that only ever measures the passing case cannot fail, and a
      gate that cannot fail is worse than no gate (R1527). This is what makes
      (B) mean something.

  (D) **Depth invariance.** Scrolling to the far end of a million-row model
      does not move the census either. A binding can window correctly at the
      top and still materialise a prefix.

      Two things move the window legitimately, and neither is scale:

        * The top of a list is a *smaller* window than the middle — there are
          no rows above row 0 to overscan into (measured R1538: 63 nodes at
          rest, 75 once scrolled).
        * An offset that lands exactly on a row boundary needs one fewer
          partial row than one that does not (75, or 72 when aligned).

      So the guard asserts neither "always equal" — that would be a claim
      about which offsets the flicks happen to land on. It asserts the
      **peak** is model-independent, which is the property: a million-row
      model must never paint more than a ten-thousand-row one, and the
      at-rest-to-peak difference must be the same for both, because an edge
      effect is a constant of the viewport and anything that grows with the
      model is not one.

  (E) The census is not derived from a duration, and not from the frame
      number: two frames with the same census can differ in time, and the
      arms differ in census while the header and the scrollbar are identical.

  (F) The other three view producers report their own node totals
      (`produce.nodes_total`, `mirror.nodes_total`), so an agent can price the
      work its OWN call caused — `passes_total` counts loop iterations, which
      is the same number for a forty-node scene and a forty-thousand-node one.

ZERO-FLAKE: not one assertion names a microsecond, a frame rate, or a machine.
Every claim is a count, an ordering, or a presence. Frames are driven by the
window's own `frame_count`, never by a sleep.

Run from the workspace root:
    cargo build -p hello-scene-scale --release
    python3 tools/demos/r1538_scene_scale.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

APP = "hello-scene-scale"
LIST_TAG = "scale"
EXT = f"/{LIST_TAG}/external"

# The binding's own ladder. The virtual arm accepts all of it; the eager arm
# is capped, which the binding publishes rather than making a client discover.
LADDER = [100, 1_000, 10_000, 100_000, 1_000_000]


def drive_frame(tf: RpcSubprocess, baseline: int, desc: str) -> dict:
    """Drive real paints until `frame_count` passes `baseline`, then read.

    `scene/screenshot` forces a real view + layout + encode + submit through
    the live pipeline, which is the only thing that records a frame. A census
    read off a producer pass would describe a scene nobody painted.
    """

    def advanced() -> bool:
        try:
            if int(tf.frame_timings()["frame_count"]) > baseline:
                return True
        except RpcError:
            pass
        tf.request("scene/screenshot", {"path": ""})
        return False

    wait_until(advanced, desc=desc)
    return tf.frame_timings()


# One flick, in logical pixels. Far larger than any viewport, so a single one
# lands in the interior of even the smallest model on the ladder and the
# clamp does the rest — no arithmetic against a row pitch, which would make
# this guard depend on the binding's styling.
DEEP_SCROLL_PX = 40_000.0


# Enough to reach offset 0 from the bottom of the LARGEST model on the ladder
# (a million rows of 28px is ~28M), so the clamp lands it exactly at the top.
# One flick, not a loop: a loop would need a termination condition, and the
# condition would be the offset, which is the thing being reset.
TOP_SCROLL_PX = 40_000_000.0


def scroll_to_top(tf: RpcSubprocess) -> None:
    """Return the list to offset 0.

    The scroll state outlives a model change, so a measurement taken "at rest"
    has to PUT the list at rest rather than assume it is. The first draft of
    this guard scrolled back by one flick's worth and read 75 where a fresh
    boot reads 63 — it had reset nothing, and the at-rest baseline was another
    interior sample wearing the label.
    """
    tf.wheel(path=LIST_TAG, pixels=(0.0, -TOP_SCROLL_PX))
    tf.tick(0.016)


def set_rows(tf: RpcSubprocess, rows: int) -> None:
    tf.intervene(f"{EXT}/rows", rows)
    assert_eq(tf.query(f"{EXT}/rows"), rows, f"the model took rows={rows}")


def census_of(ft: dict) -> tuple[int, int, int]:
    last = ft["last"]
    return last["scene_nodes"], last["layout_nodes"], last["encode_nodes"]


def access_of(ft: dict) -> int:
    return ft["last"]["access_nodes"]


def assert_wire_shape(ft: dict, label: str) -> None:
    """Present, typed, and mutually coherent — before any of it is believed."""
    last, win = ft["last"], ft["window"]
    for field in ("scene_nodes", "layout_nodes", "encode_nodes", "access_nodes"):
        assert field in last, f"{label}: `last.{field}` missing from the wire"
        assert isinstance(last[field], int) and not isinstance(last[field], bool), (
            f"{label}: `last.{field}` must be an integer count, got {last[field]!r}"
        )
    for field in (
        "max_scene_nodes",
        "max_layout_nodes",
        "max_encode_nodes",
        "max_access_nodes",
    ):
        assert field in win, (
            f"{label}: `window.{field}` missing — the peak is what an upper-bound "
            f"claim reads, and a mean would hide the one frame that built the model"
        )

    assert last["scene_nodes"] > 0, (
        f"{label}: a painted frame has a root, so its tree cannot be empty"
    )
    assert last["layout_nodes"] >= last["scene_nodes"], (
        f"{label}: the settle loop measured {last['layout_nodes']} nodes but "
        f"produced a {last['scene_nodes']}-node tree — the sum cannot be below "
        f"its own last term"
    )
    assert last["access_nodes"] >= 1, (
        f"{label}: the accessibility walk produced no nodes. This binding "
        f"declares an AT tree on every frame, so `0` is not a small tree — it "
        f"is an unmeasured walk, and section (B)'s flatness assertion would "
        f"pass on a column of zeros"
    )
    assert last["encode_nodes"] >= 1, (
        f"{label}: the encode walked no nodes at all. A paint enters at least "
        f"its root, so `0` here is not a perfectly-served frame — it is an "
        f"absent measurement wearing the shape of an excellent one, which is "
        f"exactly what a count below the tree size would otherwise look like"
    )
    assert last["encode_nodes"] <= last["scene_nodes"], (
        f"{label}: the encode walked {last['encode_nodes']} nodes of a "
        f"{last['scene_nodes']}-node tree — it cannot enter a node that is not there"
    )
    for peak, cur in (
        ("max_scene_nodes", "scene_nodes"),
        ("max_layout_nodes", "layout_nodes"),
        ("max_encode_nodes", "encode_nodes"),
        ("max_access_nodes", "access_nodes"),
    ):
        assert win[peak] >= last[cur], (
            f"{label}: window.{peak}={win[peak]} is below this frame's "
            f"{cur}={last[cur]} — the fold does not include the sample it rode with"
        )


def body() -> None:
    # ── (A) the census exists and is coherent ───────────────────────────────
    with RpcSubprocess(APP, boot_grace=1.5) as tf:
        cap = tf.query(f"{EXT}/max_eager_rows")
        assert isinstance(cap, int) and cap > 0, (
            f"the binding must publish its eager ceiling rather than making a "
            f"client discover it by being refused; got {cap!r}"
        )

        base = int(tf.frame_timings()["frame_count"])
        ft = drive_frame(tf, base, "boot frame")
        assert_wire_shape(ft, "boot")

        # ── (B) SCALE INVARIANCE — the round's claim ────────────────────────
        # One process, one window, one cache. The only thing that changes is
        # how big the model says it is.
        virtual_census: dict[int, tuple[int, int, int]] = {}
        virtual_access: dict[int, int] = {}
        for rows in LADDER:
            set_rows(tf, rows)
            count = int(tf.frame_timings()["frame_count"])
            ft = drive_frame(tf, count, f"virtual arm at {rows} rows")
            assert_wire_shape(ft, f"virtual/{rows}")
            virtual_census[rows] = census_of(ft)
            virtual_access[rows] = access_of(ft)

        scene_counts = {rows: c[0] for rows, c in virtual_census.items()}
        assert len(set(scene_counts.values())) == 1, (
            f"the virtual arm's painted tree must not grow with the model. "
            f"Model {LADDER[0]} -> {LADDER[-1]} is a factor of "
            f"{LADDER[-1] // LADDER[0]}; scene_nodes went {scene_counts}. "
            f"THIS is the 60fps-at-scale claim, and it is a count rather than "
            f"a wall clock precisely so it can be asserted here at all"
        )
        flat = next(iter(scene_counts.values()))
        assert flat < LADDER[1], (
            f"the window holds {flat} nodes, which is not a WINDOW of a "
            f"{LADDER[-1]}-row model — a flat number that happens to be huge "
            f"would satisfy the invariance above and prove nothing"
        )

        assert len(set(virtual_access.values())) == 1, (
            f"the ACCESSIBILITY walk must be flat too. It is a second "
            f"traversal that runs every paint and that none of the three "
            f"paint counts can see, so a binding whose AT tree grew with the "
            f"model would satisfy every assertion above while doing O(model) "
            f"work a frame: {virtual_access}"
        )

        layout_counts = {rows: c[1] for rows, c in virtual_census.items()}
        assert len(set(layout_counts.values())) == 1, (
            f"the layout work must be flat too — a binding could window the "
            f"painted tree while still measuring the model: {layout_counts}"
        )

        # ── (A continued) the encode census spans its full range ───────────
        # `encode_nodes` well below `scene_nodes` is the cache working — but a
        # constant, or a stuck zero, looks identical on any one warm frame.
        # The window's peak is what settles it: a model change invalidates the
        # root, so some frame in the rolling window could NOT be served and
        # walked the whole tree. The census therefore has to reach both ends.
        ft = tf.frame_timings()
        win, last = ft["window"], ft["last"]
        assert_eq(
            win["max_encode_nodes"],
            win["max_scene_nodes"],
            "a frame in this window repainted after a model change, so the "
            "fragment cache could not serve its root and the encode had to "
            "walk the entire tree. A census that could not reach the top of "
            "its own range would be reporting a constant",
        )
        assert last["encode_nodes"] * 2 < win["max_encode_nodes"], (
            f"...and the other end: a steady frame walked "
            f"{last['encode_nodes']} nodes against a peak of "
            f"{win['max_encode_nodes']}. Both ends together are what make this "
            f"a measurement of the WALK rather than of the tree"
        )

        # ── (D) DEPTH INVARIANCE — the far end of a million rows ────────────
        edges: dict[int, tuple[int, int, int]] = {}
        for rows in (LADDER[2], LADDER[-1]):
            set_rows(tf, rows)
            scroll_to_top(tf)
            count = int(tf.frame_timings()["frame_count"])
            at_rest = census_of(drive_frame(tf, count, f"{rows} rows, at rest"))[0]

            deep: list[int] = []
            for step in range(6):
                tf.wheel(path=LIST_TAG, pixels=(0.0, DEEP_SCROLL_PX))
                tf.tick(0.016)
                count = int(tf.frame_timings()["frame_count"])
                ft = drive_frame(tf, count, f"{rows} rows, scrolled {step + 1}")
                assert_wire_shape(ft, f"deep/{rows}/{step + 1}")
                deep.append(census_of(ft)[0])

            assert min(deep) >= at_rest, (
                f"{rows} rows: scrolling into the middle gave as few as "
                f"{min(deep)} nodes against {at_rest} at rest — the middle has "
                f"overscan on both sides, so it cannot be the smaller window"
            )
            edges[rows] = (at_rest, min(deep), max(deep))

        (small_rest, small_lo, small_hi) = edges[LADDER[2]]
        (large_rest, large_lo, large_hi) = edges[LADDER[-1]]

        assert_eq(
            large_hi,
            small_hi,
            f"a {LADDER[-1]}-row model scrolled through its interior never "
            f"paints more than a {LADDER[2]}-row one. THIS is depth invariance "
            f"— (B) showed the model can grow 10,000x at rest, and this shows "
            f"it stays true at the far end of the scroll, where a binding that "
            f"materialises a prefix would give itself away",
        )
        assert_eq(
            large_lo,
            small_lo,
            "and the floor matches too, so the peak above is not one model "
            "being consistently larger with a lucky maximum",
        )
        assert_eq(
            large_hi - large_rest,
            small_hi - small_rest,
            "the at-rest window is smaller because row 0 has nothing above it "
            "to overscan into. That edge effect is a constant of the VIEWPORT, "
            "so it must be identical at both model sizes — one that grew with "
            "the model would be a prefix being materialised, wearing an edge "
            "effect's clothes",
        )
        assert large_hi - large_lo < large_rest, (
            f"the interior window varied {large_lo}..{large_hi} while scrolling. "
            f"A row-aligned offset needs one fewer partial row than a "
            f"misaligned one, so some variation is expected — but it must stay "
            f"far below the window itself ({large_rest}), or it is not "
            f"alignment jitter, it is the model leaking in"
        )

        # ── (F) the other producers state their size too ────────────────────
        before = tf.frame_timings()
        tf.snapshot()
        after = tf.frame_timings()
        assert "nodes_total" in after["produce"], (
            "`produce.nodes_total` missing — an agent on the §2 #2 path must be "
            "able to price the work its own calls caused, and `passes_total` "
            "counts loop iterations, which is the same for any scene size"
        )
        assert "nodes_total" in after["mirror"], "`mirror.nodes_total` missing"
        assert after["produce"]["nodes_total"] >= before["produce"]["nodes_total"], (
            "a cumulative total cannot go backwards"
        )

    # ── (C) THE GUARD CAN FAIL — the negative control ───────────────────────
    # A fresh process, because the eager arm is entered from a small model and
    # nothing about (B) should be able to leak into it.
    with RpcSubprocess(APP, boot_grace=1.5) as tf:
        cap = int(tf.query(f"{EXT}/max_eager_rows"))
        rungs = [n for n in LADDER if n <= cap]
        assert len(rungs) >= 2, (
            f"the negative control needs two rungs below the eager cap {cap} "
            f"to show growth; ladder has {rungs}"
        )

        set_rows(tf, rungs[0])
        tf.intervene(f"{EXT}/eager", True)
        assert_eq(tf.query(f"{EXT}/eager"), True, "the eager arm is entered")

        eager_census: dict[int, tuple[int, int, int]] = {}
        eager_access: dict[int, int] = {}
        for rows in rungs:
            set_rows(tf, rows)
            count = int(tf.frame_timings()["frame_count"])
            ft = drive_frame(tf, count, f"eager arm at {rows} rows")
            assert_wire_shape(ft, f"eager/{rows}")
            eager_census[rows] = census_of(ft)
            eager_access[rows] = access_of(ft)

        small, large = eager_census[rungs[0]], eager_census[rungs[-1]]
        grew = large[0] - small[0]
        expected = rungs[-1] - rungs[0]
        assert grew >= expected, (
            f"the eager arm built {grew} more nodes for {expected} more rows "
            f"({rungs[0]} -> {rungs[-1]}, census {small[0]} -> {large[0]}). "
            f"If the census cannot SEE an unwindowed list, then section (B) "
            f"above measured a constant and asserted nothing"
        )
        assert large[1] >= large[0], "eager: the layout sum still bounds the tree"
        assert eager_access[rungs[-1]] > eager_access[rungs[0]], (
            f"the eager arm is unwindowed in BOTH walks, so its AT tree must "
            f"grow with the model too ({eager_access}). If it did not, the "
            f"a11y half of section (B) would be asserting a constant"
        )

        # The cap refuses rather than clamps — a guard reading a clamped value
        # would believe it was measuring a model the binding does not hold.
        too_big = next(n for n in LADDER if n > cap)
        try:
            tf.intervene(f"{EXT}/rows", too_big)
            raise AssertionError(
                f"the eager arm accepted {too_big} rows above its stated cap "
                f"{cap}; a silent clamp is indistinguishable from a lie"
            )
        except RpcError:
            pass
        assert_eq(
            tf.query(f"{EXT}/rows"),
            rungs[-1],
            "and the refused write left the model where it was",
        )

        # ── (E) the census is a property of the TREE, not of the arm ────────
        # Leave the eager arm at the same row count. The model is identical,
        # the header and the scrollbar are identical, and the census must drop
        # to the window — so the number is reading what the frame built and
        # not a flag it was handed.
        tf.intervene(f"{EXT}/eager", False)
        count = int(tf.frame_timings()["frame_count"])
        ft = drive_frame(tf, count, f"back to virtual at {rungs[-1]} rows")
        back = census_of(ft)
        assert back[0] < large[0], (
            f"the same {rungs[-1]}-row model painted {large[0]} nodes eagerly "
            f"and {back[0]} windowed — if these matched, the census would be "
            f"reading the model rather than the tree"
        )

        # And the fragment cache's share of it: a steady repaint of an
        # unchanged scene must walk far less than the tree it replays. This is
        # the thing `scene/cache_stats`' hit rate cannot state — replaying two
        # enormous fragments and two tiny ones are both 100%.
        count = int(tf.frame_timings()["frame_count"])
        steady = census_of(drive_frame(tf, count, "steady repaint"))
        assert steady[2] < steady[0], (
            f"a repaint of an unchanged scene walked {steady[2]} of "
            f"{steady[0]} nodes — the §5.16 fragment cache short-circuits a "
            f"hit's whole subtree, so anything else means it served nothing"
        )
        assert_eq(
            steady[0],
            back[0],
            "and the tree itself did not change under the repaint",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1538 the frame states how much of the scene it walked", body))
