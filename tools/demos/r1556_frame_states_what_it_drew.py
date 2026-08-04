#!/usr/bin/env python3
"""R1556 §5.16 §5.27 §2 #2 — the frame states the work it DREW, not just the
nodes it walked.

R1538 gave this axis its node census and used it to state the claim the axis is
named for: per-frame work is bounded by what is *visible*. A count is the right
shape for that claim — unlike a duration it does not move with the host — but it
prices every node at one. **A `Container` is one node; a `Text` leaf holding
four thousand glyphs is one node.** So two frames can report a byte-identical
census and hand the GPU two orders of magnitude of different work, and the guard
R1538 built to bound per-frame work could not see the difference.

`last.draw` (with a `window.max_draw` peer) is the other half: what was drawn,
counted in the units a 2D vector renderer is charged in —

  * `draws`         — encoded draw commands (the peer of a draw-call count)
  * `paths`         — filled / stroked shapes, plus one per closed clip layer
  * `path_segments` — line and curve segments across them: the GEOMETRIC size
  * `layers`        — clip / blend layers, whose cost is per-layer, not per-item
  * `glyph_runs`    — shaped runs handed to the rasterizer, one text draw each
  * `glyphs`        — positioned glyphs across them: the TEXT size

It is read off the scene that was **submitted**, so a subtree the §5.16 fragment
cache replayed counts exactly like one encoded this frame — there is no path by
which drawn work escapes the count.

This demo asserts:

  (A) The census is on the wire, typed, complete, and internally coherent, and
      `rpc/schema` DESCRIBES it — the published shape and the live response's
      key set are compared against each other (R1539's discipline: a response
      shape nobody checks is a comment).

  (B) A painted frame actually draws: paths, segments and glyphs are all
      non-zero. `0` here would not be a perfectly-efficient frame, it would be
      an absent measurement wearing the shape of an excellent one — and every
      invariance assertion below would then pass on a column of zeros.

  (C) **Scale invariance in DRAW units.** Growing the model by a factor of
      10,000 does not move the glyph count, the segment count or the draw-command
      count. This is R1538's claim made in the units the GPU is actually charged
      in, which is strictly stronger than the node form: the node census stays
      flat under (E) too, and (E) is a frame doing sixty times the work.

  (D) **The guard can fail.** The eager arm builds one node per row, and there
      the draw census MUST grow with the model. A guard that only ever measures
      the passing case cannot fail (R1527).

  (E) **THE ROUND'S OWN CASE — a node count is not a cost.** With `rows` and the
      arm held fixed, widening each row's label 24 → 1,536 characters leaves
      EVERY node count exactly where it was — `scene_nodes`, `layout_nodes`,
      `encode_nodes`, `access_nodes`, all four identical — while the frame's
      glyph count grows by more than an order of magnitude. Nothing on this wire
      could state that before this round.

  (F) **Text and geometry are disjoint.** A shaped run is encoded as positioned
      glyphs and its outlines become paths downstream of the encoding, so the
      width ladder moves `glyphs` and leaves `paths` alone. That is what lets a
      frame's text cost and its vector cost be read apart instead of summed into
      one number from which neither survives.

  (G) **Walked is not drawn.** Repainting an unchanged scene collapses
      `encode_nodes` (the fragment cache serves the tree) while the draw census
      is unchanged to the last glyph — the replayed fragments are still drawn.
      A census kept by the walker would report the second frame as drawing
      almost nothing, which is the reading a profiler must never produce.

ZERO-FLAKE: not one assertion names a microsecond, a frame rate, or a machine.
Every claim is a count, an ordering, an equality or a presence. Frames are
driven by the window's own `frame_count`, never by a sleep.

Run from the workspace root:
    cargo build -p hello-scene-scale --release
    python3 tools/demos/r1556_frame_states_what_it_drew.py
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

# The binding's two ladders. `LADDER` moves how MANY rows the model has;
# `LABEL_LADDER` moves what each row COSTS with the count held fixed.
LADDER = [100, 1_000, 10_000, 100_000, 1_000_000]
LABEL_LADDER = [24, 96, 384, 1_536]

DRAW_FIELDS = ("draws", "paths", "path_segments", "layers", "glyph_runs", "glyphs")
NODE_FIELDS = ("scene_nodes", "layout_nodes", "encode_nodes", "access_nodes")


def drive_frame(tf: RpcSubprocess, baseline: int, desc: str) -> dict:
    """Drive real paints until `frame_count` passes `baseline`, then read.

    `scene/screenshot` forces a real view + layout + encode + submit through the
    live pipeline, which is the only thing that records a frame — and, for this
    round, the only thing that produces a submitted scene to census. A read off
    a producer pass would describe a scene nobody drew.
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


def next_frame(tf: RpcSubprocess, desc: str) -> dict:
    return drive_frame(tf, int(tf.frame_timings()["frame_count"]), desc)


def draw_of(ft: dict) -> dict:
    return ft["last"]["draw"]


def nodes_of(ft: dict) -> dict:
    return {f: ft["last"][f] for f in NODE_FIELDS}


def set_rows(tf: RpcSubprocess, rows: int) -> None:
    tf.intervene(f"{EXT}/rows", rows)
    assert_eq(tf.query(f"{EXT}/rows"), rows, f"the model took rows={rows}")


def set_label_chars(tf: RpcSubprocess, chars: int) -> None:
    tf.intervene(f"{EXT}/label_chars", chars)
    assert_eq(tf.query(f"{EXT}/label_chars"), chars, f"the model took chars={chars}")


def assert_wire_shape(ft: dict, label: str) -> None:
    """Present, typed, complete and mutually coherent — before any belief."""
    last, win = ft["last"], ft["window"]
    assert "draw" in last, (
        f"{label}: `last.draw` missing from the wire. The node census is the "
        f"size of a TREE; without this the frame states no cost at all"
    )
    assert "max_draw" in win, (
        f"{label}: `window.max_draw` missing — the guarded property is an upper "
        f"bound, and a mean cannot state one"
    )
    draw, peak = last["draw"], win["max_draw"]
    assert_eq(
        sorted(draw),
        sorted(DRAW_FIELDS),
        f"{label}: `last.draw` key set is the published one",
    )
    assert_eq(
        sorted(peak),
        sorted(DRAW_FIELDS),
        f"{label}: `window.max_draw` answers the same keys as `last.draw`",
    )
    for field in DRAW_FIELDS:
        for group, value in (("last.draw", draw[field]), ("window.max_draw", peak[field])):
            assert isinstance(value, int) and not isinstance(value, bool), (
                f"{label}: `{group}.{field}` must be an integer count, got {value!r}"
            )
            assert value >= 0, f"{label}: `{group}.{field}` is negative: {value}"
        assert peak[field] >= draw[field], (
            f"{label}: window.max_draw.{field}={peak[field]} is below this "
            f"frame's {draw[field]} — the fold does not include the sample it "
            f"rode with"
        )


def assert_frame_actually_drew(draw: dict, label: str) -> None:
    """(B) Zero is not a good frame here, it is an unmeasured one."""
    for field in ("draws", "paths", "path_segments", "glyph_runs", "glyphs"):
        assert draw[field] > 0, (
            f"{label}: `last.draw.{field}` is 0. This binding paints filled "
            f"rows carrying text, so a zero is an absent measurement wearing "
            f"the shape of an excellent one — and every invariance assertion "
            f"below would pass on a column of zeros"
        )
    assert draw["layers"] >= 1, (
        f"{label}: the list is inside a scroll viewport, which pushes a clip "
        f"layer; `layers=0` means the clip never reached the encoding"
    )
    # Each field must also be DISTINGUISHABLE from its neighbours, or a
    # transposed read would satisfy everything above: six counts of the same
    # frame are all plausible in each other's slots. These are the relations
    # that hold only when each field means what it says.
    assert draw["path_segments"] > draw["paths"] * 2, (
        f"{label}: {draw['path_segments']} segments across {draw['paths']} paths "
        f"— the smallest shape this binding encodes is a filled rect, which is "
        f"four segments, so a segment count near the path count is a path count "
        f"wearing another name"
    )
    assert draw["glyphs"] > draw["glyph_runs"] * 5, (
        f"{label}: {draw['glyphs']} glyphs across {draw['glyph_runs']} runs — "
        f"every row label here is many characters long, so these two cannot be "
        f"close, and a run with no glyphs in it is not a run"
    )
    assert draw["draws"] > draw["paths"], (
        f"{label}: {draw['draws']} draw commands for {draw['paths']} paths — "
        f"every path is issued by a command AND text issues commands that encode "
        f"no path, so a frame with glyphs in it must command more than it shapes"
    )


def body() -> None:
    with RpcSubprocess(APP, boot_grace=1.5) as tf:
        # ── (A) the census is on the wire AND is described there ────────────
        base = int(tf.frame_timings()["frame_count"])
        ft = drive_frame(tf, base, "boot frame")
        assert_wire_shape(ft, "boot")

        schema = tf.request("rpc/schema", {}).result
        types = {t["name"]: t for t in schema["types"]}
        assert "FrameTimingsDraw" in types, (
            "the draw census must be in `rpc/schema`. R1539 exists because a "
            "published response shape nobody checks is a comment, and growing "
            "one then looks like an ordinary struct edit"
        )
        published = [f["name"] for f in types["FrameTimingsDraw"]["shape"]["fields"]]
        assert_eq(
            sorted(published),
            sorted(DRAW_FIELDS),
            "the published census names exactly the keys the wire answers",
        )
        assert_eq(
            sorted(published),
            sorted(draw_of(ft)),
            "…and the LIVE response agrees with the published census",
        )
        for group, key, ref in (
            ("FrameTimingsLast", "draw", "FrameTimingsDraw"),
            ("FrameTimingsWindow", "max_draw", "FrameTimingsDraw"),
        ):
            field = next(
                f for f in types[group]["shape"]["fields"] if f["name"] == key
            )
            assert_eq(field["of"], ref, f"{group}.{key} names its nested type")

        # ── (B) a painted frame actually draws ──────────────────────────────
        assert_frame_actually_drew(draw_of(ft), "boot")

        # ── (C) SCALE INVARIANCE, in the units the GPU is charged in ────────
        # One process, one window, one cache. Only the model size moves.
        by_rows: dict[int, dict] = {}
        for rows in LADDER:
            set_rows(tf, rows)
            ft = next_frame(tf, f"virtual arm at {rows} rows")
            assert_wire_shape(ft, f"virtual/{rows}")
            assert_frame_actually_drew(draw_of(ft), f"virtual/{rows}")
            by_rows[rows] = draw_of(ft)

        # Five of the six units are FLAT across four orders of magnitude of
        # model: the windowed list draws the same shapes, the same clip layer
        # and the same number of text draws whether it is backed by a hundred
        # rows or a million.
        for field in ("draws", "paths", "path_segments", "layers", "glyph_runs"):
            seen = {rows: d[field] for rows, d in by_rows.items()}
            assert len(set(seen.values())) == 1, (
                f"the virtual arm's DRAWN work must not grow with the model. "
                f"Model {LADDER[0]} -> {LADDER[-1]} is a factor of "
                f"{LADDER[-1] // LADDER[0]}; draw.{field} went {seen}. R1538 "
                f"stated this claim in nodes; this states it in what the GPU "
                f"actually executes, which is the claim that was being made"
            )

        # The sixth is not flat, and what moves it is the whole point of having
        # a census this precise: the header button *displays the number*, so a
        # ten-times-bigger model is one more DIGIT of visible text. The growth
        # is therefore log10 of the model and not proportional to it — and it is
        # asserted as an exact identity rather than a bound, because the census
        # can resolve a single glyph and a looser assertion would let a real
        # per-row leak hide inside the slack.
        base_glyphs = by_rows[LADDER[0]]["glyphs"]
        for rows, drawn in by_rows.items():
            extra_digits = len(str(rows)) - len(str(LADDER[0]))
            assert_eq(
                drawn["glyphs"] - base_glyphs,
                extra_digits,
                f"at {rows} rows the frame must draw exactly {extra_digits} more "
                f"glyph(s) than at {LADDER[0]} — the digits the header shows, "
                f"and nothing else. Per-frame drawn text is O(log(model)) here "
                f"because the model's SIZE is on screen; anything O(model) is a "
                f"row that escaped the window",
            )

        # ── (D) the guard can fail: the eager arm's work tracks the model ───
        set_rows(tf, LADDER[0])
        tf.intervene(f"{EXT}/eager", True)
        assert_eq(tf.query(f"{EXT}/eager"), True, "entered the eager arm")
        ft = next_frame(tf, "eager arm at 100 rows")
        eager_small = draw_of(ft)

        set_rows(tf, 1_000)
        ft = next_frame(tf, "eager arm at 1,000 rows")
        eager_big = draw_of(ft)

        for field in ("glyphs", "paths", "draws"):
            assert eager_big[field] > eager_small[field] * 5, (
                f"the eager arm builds one node per row, so ten times the model "
                f"must draw far more: draw.{field} went {eager_small[field]} -> "
                f"{eager_big[field]}. Without this, section (C) is a guard that "
                f"cannot fail"
            )
        assert eager_big["glyphs"] > by_rows[LADDER[-1]]["glyphs"] * 10, (
            f"a thousand eager rows must out-draw a MILLION windowed ones "
            f"({eager_big['glyphs']} vs {by_rows[LADDER[-1]]['glyphs']} glyphs) "
            f"— the whole point of windowing, stated in drawn work"
        )

        tf.intervene(f"{EXT}/eager", False)
        assert_eq(tf.query(f"{EXT}/eager"), False, "back on the virtual arm")

        # ── (E) THE ROUND'S CASE — same nodes, different cost ───────────────
        set_rows(tf, LADDER[1])
        by_width: dict[int, tuple[dict, dict]] = {}
        for chars in LABEL_LADDER:
            set_label_chars(tf, chars)
            ft = next_frame(tf, f"label width {chars}")
            assert_wire_shape(ft, f"width/{chars}")
            by_width[chars] = (nodes_of(ft), draw_of(ft))

        narrow_nodes, narrow_draw = by_width[LABEL_LADDER[0]]
        wide_nodes, wide_draw = by_width[LABEL_LADDER[-1]]
        assert_eq(
            wide_nodes,
            narrow_nodes,
            "EVERY node count must be identical across the width ladder — this "
            "is the premise, and if the tree moved at all the comparison below "
            "would be attributable to something other than per-node cost",
        )
        for chars, (nodes, _) in by_width.items():
            assert_eq(nodes, narrow_nodes, f"width {chars} moved a node count")

        ratio = LABEL_LADDER[-1] // LABEL_LADDER[0]
        assert wide_draw["glyphs"] > narrow_draw["glyphs"] * (ratio // 4), (
            f"…while the frame's DRAWN work tracks the width: draw.glyphs went "
            f"{narrow_draw['glyphs']} -> {wide_draw['glyphs']} across a {ratio}x "
            f"widening. This is the case a node census reports as no change at "
            f"all, and it is why 'nodes' was never the same claim as 'work'"
        )
        widths = [by_width[c][1]["glyphs"] for c in LABEL_LADDER]
        assert all(b > a for a, b in zip(widths, widths[1:])), (
            f"and it must be monotone across the whole ladder, got {widths}"
        )

        # ── (F) text and geometry are disjoint axes ────────────────────────
        assert_eq(
            (wide_draw["paths"], wide_draw["path_segments"]),
            (narrow_draw["paths"], narrow_draw["path_segments"]),
            "widening the labels must not encode one extra PATH or SEGMENT: a "
            "shaped run is encoded as positioned glyphs and its outlines become "
            "paths downstream, so text and geometry are disjoint here rather "
            "than summed — which is what lets either be read on its own",
        )
        assert_eq(
            wide_draw["layers"],
            narrow_draw["layers"],
            "…and no extra clip layer either: the viewport is the same viewport",
        )

        # ── (G) walked is not drawn ────────────────────────────────────────
        set_label_chars(tf, LABEL_LADDER[0])
        first = next_frame(tf, "settle before the repaint pair")
        second = next_frame(tf, "repaint of an unchanged scene")
        assert_eq(
            draw_of(second),
            draw_of(first),
            "repainting an unchanged scene draws exactly the same work — a "
            "replayed fragment is still drawn, so a census kept by the WALKER "
            "would report this frame as drawing almost nothing",
        )
        assert second["last"]["encode_nodes"] < second["last"]["scene_nodes"], (
            f"premise: the repaint must be served by the fragment cache, so the "
            f"walk is shorter than the tree "
            f"({second['last']['encode_nodes']} vs "
            f"{second['last']['scene_nodes']})"
        )
        assert draw_of(second)["glyphs"] > second["last"]["scene_nodes"], (
            f"and the pairing that makes both numbers worth having: this frame "
            f"walked {second['last']['encode_nodes']} nodes of a "
            f"{second['last']['scene_nodes']}-node tree and drew "
            f"{draw_of(second)['glyphs']} glyphs. Neither number alone says "
            f"whether that is a cache doing its job"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1556 the frame states the work it drew", body))
