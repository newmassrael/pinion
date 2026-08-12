#!/usr/bin/env python3
"""R1531 §5.36 §5.7 §2 #2 — the paint replays a draw list it does not rebuild.

The §5.36 `LayoutCache` cached what parley *shaped*. It did not cache what
a painter *draws*, and those are not the same thing: to put a shaped
layout on screen the painter walks `lines() -> items() -> GlyphRun`, reads
each run's font / size / brush, and runs `positioned_glyphs()`, which
accumulates every glyph's pen advance into an absolute position. That walk
is a pure function of the layout — the same layout yields the same list
forever — and until R1531 it ran on every paint of every text leaf.

Measured on this box (release, 1,200 text leaves, both caches warm, the
"before" column a real run of the pre-change code and not an estimate):

                                        pre-R1531    R1531
    steady-state text paint, per frame    1,489 us    480 us

3.1x, or 1.33 us -> 0.40 us per leaf. The walk costs more than its own
standalone measurement (709 us) suggests, because interleaving it with the
encoder's work costs both of them cache locality — which is the case for
hoisting it out entirely rather than merely making it cheaper.

The shape is the canonical one. Skia caches an `SkTextBlob`, the toolkit a
glyph run (and the static text that holds one): shaping produces a
layout, a second cheap step produces a replayable positioned glyph list,
and the list is drawn many times per build. Here it lives in the cache
that already owns the layout — same key, same lifetime, same eviction,
because it is the second half of one derivation rather than a separate
thing to keep in sync.

## What this demo drives, and why this binding

`hello-grid-nav` is a 10,000-row keyboard-navigable data grid, and its
ArrowDown is precisely the frame this round is about: a §5.16 fragment
cache MISS that is not a §5.36 shape cache miss. One row strip changes
colour, so that strip and the root above it go back into the Vello stream
— every glyph in them re-encoded — while not one string changed.

That frame was invisible to every counter the framework had. `shapes`
stays still across it by construction (nothing re-shaped), and
`scene/cache_stats` reports the fragment cache doing exactly what it
should. The cost sat in the gap between them, which is why `run_builds`
is a new field rather than a new reading of an old one.

Measured on this binding, one ArrowDown:

                          new row selected    row selected before
    fragments re-encoded          10                  10
    layouts shaped                 2                   0
    draw lists derived             1                   0

The right-hand column is the frame this round is named for. Ten fragments
go back into the stream, every glyph in them re-encoded, and the walk that
positions those glyphs runs zero times. (The left-hand column derives one
because the selection *colour* is part of the layout key today — the
label under the cursor is a different cache entry from the same label
unselected. That is a registered debt of its own, not this round's, and
one derivation against ten re-encoded fragments is what it costs.)

## Verification scope (>= 30 assertions, sections A-G)

  (A) `scene/text_cache_stats` typed surface — every documented field
      present with the documented type, `run_builds` among them, and
      bounded by `shapes` (a layout must be shaped before it is drawn).
  (B) It is a different question from `scene/cache_stats`. The fragment
      cache misses on the very frames the draw list must NOT be rebuilt
      on, which is the whole reason the two counters cannot substitute
      for each other.
  (C) Boot census + idle frames — a quiet frame derives nothing.
  (D) A re-encoding frame derives at most what it newly SHAPED, never
      what it painted. It also derives strictly less, which is the
      laziness: a layout shaped to be measured builds no draw list.
  (E) THE HEADLINE — stepping back over rows selected once before
      re-encodes ten fragments and derives NOTHING. `shapes` is still
      across this frame by construction, which is exactly why the cost
      it used to carry was invisible.
  (F) NEGATIVE CONTROL — a jump to content the process has never shown
      must shape AND derive. A counter frozen for the wrong reason (a
      derivation that silently never happens, a list served for the wrong
      text) passes (D) and (E) and fails here.
  (G) Pixel witness — a frame served from the replayed draw list is
      byte-identical to the frame that derived it. This is what makes the
      saving a saving rather than a silent visual regression.
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    Png,
    RpcSubprocess,
    assert_eq,
    assert_same_picture,
    read_png_rgba8,
    run_demo,
    wait_paint_beyond,
    wait_query,
    wait_until,
)

EXAMPLE = "hello-grid-nav"
WIN = (400, 480)
TABLE_TAG = "vtbl"

# Documented wire fields and their JSON types. `run_builds` is R1531's;
# the rest are R1521's and are asserted here because a new field must not
# displace an old one.
FIELDS = (
    ("shapes", int),
    ("run_builds", int),
    ("entries", int),
    ("capacity", int),
    ("max_capacity", int),
    ("growths", int),
    ("font_scans", int),
    ("at_ceiling", bool),
)


def text_stats(tf: RpcSubprocess) -> dict:
    return tf.text_cache_stats()


def frag_stats(tf: RpcSubprocess) -> dict:
    return tf.cache_stats()


def paint_after(tf: RpcSubprocess, action, tick: bool = True) -> None:
    """Run `action` and land exactly one frame.

    `tick=False` for an action that arms its own redraw — a key event does,
    a programmatic scroll does not. Ticking anyway lands two frames on this
    binding (R1527's rig lesson), and the second is a pure cache hit that
    overwrites every per-frame observable.
    """
    before = int(frag_stats(tf)["paint_count"])
    action()
    if tick:
        tf.tick(0.016)
    wait_paint_beyond(tf, before)


def capture(tf: RpcSubprocess, name: str) -> Png:
    out = Path(tempfile.mkdtemp(prefix="pinion-r1531-")) / f"{name}.png"
    res = tf.request("scene/screenshot", {"path": "", "out_path": str(out)})
    assert res.result, f"{name}: screenshot returned no result"
    assert_eq((res.result["width"], res.result["height"]), WIN, f"{name} extent")
    assert out.exists(), f"{name}: no PNG at {out}"
    return read_png_rgba8(out)


def step_to(tf: RpcSubprocess, key: str, target: int) -> None:
    """One keyboard step, landing exactly one frame, and settle on it."""
    paint_after(tf, lambda: tf.key(path=TABLE_TAG, name=key), tick=False)
    wait_query(tf, "/external/selected", target, desc=f"{key} selects row {target}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) the typed surface ────────────────────────────────────
        st = text_stats(tf)
        for field, kind in FIELDS:
            assert field in st, f"text_cache_stats publishes `{field}`"
            assert isinstance(st[field], kind), (
                f"`{field}` is {kind.__name__}, got {type(st[field]).__name__}"
            )
        assert st["shapes"] > 0, "the boot paint shaped this grid's labels"
        assert st["run_builds"] > 0, "and drew them, so it derived draw lists"
        assert st["run_builds"] <= st["shapes"], (
            f"a layout is shaped before it is drawn, so derivations "
            f"({st['run_builds']}) cannot exceed shapes ({st['shapes']})"
        )
        assert st["font_scans"] <= 1, (
            f"R1447 invariant — at most one platform font scan, got "
            f"{st['font_scans']}"
        )
        assert_eq(
            st["at_ceiling"],
            st["capacity"] >= st["max_capacity"],
            "at_ceiling is derived from the pair, not carried",
        )

        # ── (B) a different question from the fragment cache ─────────
        frag = frag_stats(tf)
        assert "run_builds" not in frag, (
            "the fragment cache does not report draw-list derivations — the "
            "two counters answer about different caches, and this round "
            "exists in the gap between them"
        )
        assert "misses" not in st, (
            "and the shape cache reports `shapes` / `run_builds`, not a "
            "fragment miss count"
        )
        assert frag["misses"] > 0, "premise: the boot paint encoded fragments"

        # ── (C) idle frames derive nothing ───────────────────────────
        boot_shapes = st["shapes"]
        boot_builds = st["run_builds"]
        for i in range(3):
            paint_after(tf, lambda: None)
            quiet = text_stats(tf)
            assert_eq(
                quiet["shapes"], boot_shapes,
                f"idle paint {i}: an unchanged scene re-shapes nothing",
            )
            assert_eq(
                quiet["run_builds"], boot_builds,
                f"idle paint {i}: and derives no draw list either",
            )

        # ── (D) a re-encoding frame derives only what it shaped ─────
        tf.request("focus/set", {"tag": TABLE_TAG})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == TABLE_TAG,
            desc="grid owns focus",
        )
        for target in range(0, 8):
            prev_text = text_stats(tf)
            prev_frag = frag_stats(tf)
            step_to(tf, "ArrowDown", target)
            now_text = text_stats(tf)
            now_frag = frag_stats(tf)
            shaped = now_text["shapes"] - prev_text["shapes"]
            derived = now_text["run_builds"] - prev_text["run_builds"]
            encoded = now_frag["misses"] - prev_frag["misses"]
            assert encoded > 0, (
                f"premise for row {target}, and the load-bearing one: the "
                f"frame really did re-encode fragments. Without it every "
                f"claim below is about a frame that painted nothing"
            )
            assert derived <= shaped, (
                f"row {target}: {derived} draw lists derived against "
                f"{shaped} layouts shaped — a derivation without a shape "
                f"means the list was rebuilt for a layout already held, "
                f"which is the defect this round removed"
            )
            assert derived < encoded, (
                f"row {target}: {derived} derived against {encoded} "
                f"fragments re-encoded — before R1531 the walk ran for "
                f"every text leaf inside every one of them"
            )
            assert derived < shaped, (
                f"row {target}: strictly fewer derivations ({derived}) than "
                f"shapes ({shaped}) — a layout shaped to be MEASURED builds "
                f"no draw list, and this binding shapes one such per step"
            )

        # ── (E) THE HEADLINE: re-visiting derives nothing ────────────
        # Stepping back up over rows that have been selected once before.
        # Every label, in both the selected and the unselected colour, is
        # already in the cache — so the frame re-encodes ten fragments and
        # shapes nothing, which is the frame `shapes` can never see.
        for target in range(6, -1, -1):
            prev_text = text_stats(tf)
            prev_frag = frag_stats(tf)
            step_to(tf, "ArrowUp", target)
            now_text = text_stats(tf)
            encoded = frag_stats(tf)["misses"] - prev_frag["misses"]
            assert encoded > 0, (
                f"premise: stepping back to row {target} re-encoded "
                f"{encoded} fragments"
            )
            assert_eq(
                now_text["shapes"], prev_text["shapes"],
                f"row {target}: nothing re-shaped — which is precisely why "
                f"`shapes` alone could never have seen this cost",
            )
            assert_eq(
                now_text["run_builds"], prev_text["run_builds"],
                f"row {target}: and NO draw list was rebuilt, on a frame "
                f"that re-encoded {encoded} fragments. The walk is a "
                f"function of the layout, and no layout changed",
            )

        # ── (F) NEGATIVE CONTROL: new content must derive ────────────
        # `End` selects row 9,999 and scrolls there, so every visible label
        # is a string this process has never shaped. A `run_builds` frozen
        # because derivation silently stopped happening — or because the
        # cache answers with somebody else's list — passes (D) and (E)
        # and fails here, and (F) is what would catch the second case in pixels.
        pre_jump = text_stats(tf)
        paint_after(tf, lambda: tf.key(path=TABLE_TAG, name="End"), tick=False)
        wait_query(tf, "/external/selected", 9999, desc="End selects the last row")
        jumped = text_stats(tf)
        new_shapes = jumped["shapes"] - pre_jump["shapes"]
        new_builds = jumped["run_builds"] - pre_jump["run_builds"]
        assert new_shapes > 0, (
            f"a jump across the dataset shapes strings never seen, got "
            f"{new_shapes}"
        )
        assert new_builds > 0, (
            f"and derives a draw list for each one it draws, got "
            f"{new_builds} — this is the assertion that separates 'the list "
            f"is reused' from 'the list is never built'"
        )
        assert new_builds <= new_shapes, (
            f"still bounded by shaping: {new_builds} derivations against "
            f"{new_shapes} shapes"
        )

        # ── (G) pixel witness: replay paints what derivation painted ──
        tf.key(path=TABLE_TAG, name="Home")
        wait_query(tf, "/external/selected", 0, desc="Home returns to the top")
        paint_after(tf, lambda: None)
        step_to(tf, "ArrowDown", 1)
        step_to(tf, "ArrowDown", 2)
        paint_after(tf, lambda: None)
        derived_at = text_stats(tf)
        first_visit = capture(tf, "row2_first")
        # ★ R1664 — the rasteriser's own noise floor, measured HERE: one
        # unchanged screen captured twice, nothing in between. On this host's
        # GPU that is zero and the assertion below is byte-identity; under the
        # software Vulkan the CI sweep runs on it is a handful of bytes of
        # sub-pixel glyph coverage, which is what made this demo red there for
        # many runs while passing locally.
        control = [first_visit] + [capture(tf, f"row2_control{k}") for k in range(3)]

        # Leave and come back. The labels on the return trip are served
        # from draw lists derived on the way in, and the counter says so.
        for target in (3, 4, 5):
            step_to(tf, "ArrowDown", target)
        for target in (4, 3, 2):
            step_to(tf, "ArrowUp", target)
        paint_after(tf, lambda: None)
        replayed_at = text_stats(tf)
        second_visit = capture(tf, "row2_second")

        assert_eq(
            replayed_at["run_builds"], derived_at["run_builds"],
            "the return trip derived nothing — every label it painted was "
            "replayed from the list built on the way in",
        )
        assert_eq(
            (second_visit.width, second_visit.height),
            (first_visit.width, first_visit.height),
            "both captures are the same surface",
        )
        floor = assert_same_picture(
            control,
            (first_visit, second_visit),
            "the replayed frame is the same picture as the one that derived "
            "the list — a transcription slip in the derivation (a swapped x/y, "
            "a run dropped, run-relative positions kept where layout-absolute "
            "were meant) differs exactly here, and nowhere in the counters",
        )
        print(f"[demo] rasteriser self-disagreement this run: {floor} per channel")
        final = text_stats(tf)
        assert final["run_builds"] <= final["shapes"], (
            "the bound holds at the end as it did at the start"
        )
        assert_eq(
            final["font_scans"], st["font_scans"],
            "and none of this re-enumerated the platform fonts",
        )


if __name__ == "__main__":
    run_demo("R1531 §5.36 — the paint replays its draw list", body)
