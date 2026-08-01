#!/usr/bin/env python3
"""R1520 §5.16 §2 #7 — the paint-fragment cache survives a scroll.

Closes the R682+1 carry. R682 landed the §5.16 fragment cache with its
boundary constrained to `Affine::IDENTITY`, which made the inherited
transform a non-axis of the cache key by forbidding it. A `Scene::Scroll`
hands its content a translation, so every cacheable container under a
scrolled subtree was skipped: a scrolling list re-encoded every node,
every frame. R682's own doc registered the lift as a follow-up. What the
constraint cost was never measured until now.

R1520 stores each fragment encoded at `IDENTITY` and supplies the
inherited transform to `vello::Scene::append`, which pre-multiplies every
transform in the appended encoding. That makes `paint_hash` a *complete*
key rather than one that is merely safe under a side condition, and the
placement equivalence is exact: every paint site composes the inherited
transform on the left, and `T * (IDENTITY * local) == T * local`.

## What this demo measures, and against what

`hello-virtual-list` is the Model/View-at-scale shape (10,000 rows behind
a windowed `ScrollNode` — the asset browser / scene outliner / log view).
Numbers below are measured on this binding, before and after the change:

                              pre-R1520   post-R1520
    cacheable fragments             4          36
    per scrolled frame: hits        1          19
    per scrolled frame: misses      2           6

Four. The entire binding offered four cacheable containers, because
everything inside the list lived under a scroll translation. The
assertions are pinned between the two columns so a revert fails them.

## Verification scope (>= 30 assertions, sections A-G)

  (A) `scene/cache_stats` typed surface — every documented field
      present with the documented type, hit_rate consistent with
      hits/misses.
  (B) Boot census — the first paint stores a fragment per cacheable
      container. `entries` clears 20 (pre-R1520: 4).
  (C) Idle steady state — an unchanged re-paint hits the root alone:
      hits advance by exactly one per paint and misses do not move.
      R1527 corrected what this section asserted about the live set —
      the sweep used to collapse it to the one consulted root, evicting
      35 fragments the root had just replayed.
  (D) Scrolling reuse — across eight offsets, each frame reuses more
      fragments than it encodes (pre-R1520: 1 hit vs 2 misses, every
      frame, forever).
  (E) Reuse ratio over the whole scroll run clears 0.6 (pre: 0.33).
  (F) Pixel witness — returning to a previously visited offset paints a
      framebuffer byte-identical to the first visit. A cache *hit*
      renders what the encode-path *miss* rendered; a fragment placed at
      the wrong transform, or a stale one served across offsets, differs
      here and nowhere else in this demo.
  (G) Damage region — published on a frame that missed, absent on a
      frame that was pure hit, and BOUNDED BY THE SURFACE. Letting the
      cache reach scrolled subtrees put a scrolled container's own rect
      into this field, and a virtual list's content container is as tall
      as the data set: 320,000px inside a 460px window until the walk
      carried the accumulated clip.
"""

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    Png,
    RpcSubprocess,
    assert_eq,
    abs_rects_of,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    wait_paint_beyond,
)

EXAMPLE = "hello-virtual-list"
WIN = (360, 460)
SCROLL_TAG = "vlist_scroll"
LIST_TAG = "vlist"

# Offsets walked in (D). Chosen off the 32px row pitch so each step also
# slides the virtualized row window, i.e. the frames are not trivially
# identical scenes at different translations.
OFFSETS = (37, 74, 111, 148, 185, 222, 259, 296)

# (B) pre-R1520 = 4, post = 36. Any threshold in between discriminates;
# 20 leaves room for the binding's tree to change without re-tuning.
MIN_BOOT_ENTRIES = 20
# (E) pre-R1520 = 1/3 by construction (1 hit, 2 misses, every frame).
MIN_SCROLL_REUSE = 0.6


def stats(tf: RpcSubprocess) -> dict:
    return tf.cache_stats()


def paint_after(tf: RpcSubprocess, action) -> dict:
    """Run `action`, land exactly one frame, and return the fresh stats.

    A programmatic `scene/scroll` mutates the attached `ScrollState`
    without arming a redraw, so the tick is what turns the mutation into a
    painted frame; `wait_paint_beyond` gates on `paint_count`, the only
    counter `AppShell::render_window` advances.
    """
    before = int(stats(tf)["paint_count"])
    action()
    tf.tick(0.016)
    wait_paint_beyond(tf, before)
    return stats(tf)


def capture(tf: RpcSubprocess, name: str) -> Png:
    out = Path(tempfile.mkdtemp(prefix="pinion-r1520-")) / f"{name}.png"
    res = tf.request("scene/screenshot", {"path": "", "out_path": str(out)})
    assert res.result, f"{name}: screenshot returned no result"
    assert_eq((res.result["width"], res.result["height"]), WIN, f"{name} extent")
    assert out.exists(), f"{name}: no PNG at {out}"
    return read_png_rgba8(out)


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) the typed cache_stats surface ────────────────────────
        st = stats(tf)
        for field, kind in (
            ("hits", int),
            ("misses", int),
            ("paint_count", int),
            ("entries", int),
            ("hit_rate", float),
        ):
            assert field in st, f"cache_stats publishes `{field}`"
            assert isinstance(st[field], kind), f"`{field}` is {kind.__name__}"
        total = st["hits"] + st["misses"]
        assert total > 0, "the boot paint consulted the cache"
        assert abs(st["hit_rate"] - st["hits"] / total) < 1e-6, (
            "hit_rate is hits/(hits+misses), so a consumer never re-derives it"
        )
        assert 0.0 <= st["hit_rate"] <= 1.0, "hit_rate is a ratio"

        # ── (B) boot census: the list's containers are cacheable ─────
        rects = abs_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert SCROLL_TAG in rects, "scroll container present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")
        assert st["paint_count"] >= 1, "a real frame landed before the read"
        assert st["entries"] >= MIN_BOOT_ENTRIES, (
            f"the boot paint stores a fragment per cacheable container; "
            f"got {st['entries']}, pre-R1520 this binding offered 4 "
            f"(everything below the scroll was skipped)"
        )
        assert_eq(st["hits"], 0, "nothing to hit on the very first paint")
        assert st["misses"] == st["entries"], (
            "every first-paint miss installed exactly one fragment"
        )

        # ── (C) idle: the root hit short-circuits the whole walk ─────
        idle_prev = st
        for i in range(3):
            now = paint_after(tf, lambda: None)
            assert_eq(
                now["hits"] - idle_prev["hits"],
                1,
                f"idle paint {i}: exactly one hit (the root fragment)",
            )
            assert_eq(
                now["misses"] - idle_prev["misses"],
                0,
                f"idle paint {i}: an unchanged scene encodes nothing",
            )
            # R1527 — this asserted `1` when the round landed, which was
            # the sweep evicting every fragment the root had just
            # replayed. The hit that ends the walk is not evidence that
            # what it subsumes is dead; see the "trace step" section of
            # `FragmentCache`. The live set now survives an idle frame.
            assert now["entries"] >= MIN_BOOT_ENTRIES, (
                f"idle paint {i}: a root hit keeps the fragments it "
                f"subsumes, got {now['entries']} (pre-R1527: 1)"
            )
            assert now.get("last_damage_region") is None, (
                f"idle paint {i}: a 100% hit publishes no damage"
            )
            idle_prev = now

        # ── (D) scrolling: reuse beats re-encode, every frame ────────
        # R1527 — this used to note that the first scrolled frame "is
        # expected to miss broadly", because the idle sweep above had
        # evicted everything but the root. That was the cost of the
        # eviction stated as an expectation. The frame still misses (new
        # rows enter the window at a new offset) but now against a warm
        # cache rather than an empty one.
        prev = paint_after(tf, lambda: tf.scroll(SCROLL_TAG, to=(0, OFFSETS[0])))
        assert prev["misses"] > idle_prev["misses"], (
            "the first scrolled frame encodes the rows the offset revealed"
        )
        assert prev["hits"] > idle_prev["hits"], (
            "and reuses the ones it did not, which an idle frame no "
            "longer throws away"
        )

        scroll_hits = 0
        scroll_misses = 0
        for off in OFFSETS[1:]:
            now = paint_after(tf, lambda o=off: tf.scroll(SCROLL_TAG, to=(0, o)))
            d_hits = now["hits"] - prev["hits"]
            d_misses = now["misses"] - prev["misses"]
            scroll_hits += d_hits
            scroll_misses += d_misses
            assert d_hits > d_misses, (
                f"offset {off}: a scrolled frame reuses more than it encodes, "
                f"got {d_hits} hits vs {d_misses} misses "
                f"(pre-R1520 this was 1 vs 2 at every offset)"
            )
            assert d_hits >= 10, (
                f"offset {off}: the scrolled row fragments are reused, "
                f"got {d_hits} hits (pre-R1520: 1)"
            )
            assert now["entries"] >= MIN_BOOT_ENTRIES, (
                f"offset {off}: the live fragment set stays populated while "
                f"scrolling, got {now['entries']}"
            )
            snap = tf.snapshot(source="paint", viewport=WIN)
            assert_eq(scroll_offset(snap), off, f"offset {off} reached")
            prev = now

        # ── (E) reuse ratio across the run ───────────────────────────
        ratio = scroll_hits / (scroll_hits + scroll_misses)
        assert ratio >= MIN_SCROLL_REUSE, (
            f"scrolling reuses {ratio:.2f} of the containers it reaches; "
            f"pre-R1520 the ceiling was 0.33"
        )

        # ── (F) pixel witness: a hit paints what the miss painted ────
        # Land on a fresh offset, capture, walk away, come back, capture.
        # The return frame serves stored fragments; a transform applied at
        # the wrong place (or not at all) shows here as a moved list.
        paint_after(tf, lambda: tf.scroll(SCROLL_TAG, to=(0, 512)))
        first_visit = capture(tf, "offset512-first")
        paint_after(tf, lambda: tf.scroll(SCROLL_TAG, to=(0, 1024)))
        away = capture(tf, "offset1024")
        back_stats = paint_after(tf, lambda: tf.scroll(SCROLL_TAG, to=(0, 512)))
        second_visit = capture(tf, "offset512-again")

        assert_eq(
            (second_visit.width, second_visit.height),
            (first_visit.width, first_visit.height),
            "both captures share an extent",
        )
        assert first_visit.pixels != away.pixels, (
            "a different offset paints a different frame — otherwise the "
            "comparison below would pass on a frozen surface"
        )
        assert_eq(
            second_visit.pixels,
            first_visit.pixels,
            "returning to offset 512 paints a byte-identical framebuffer",
        )
        assert back_stats["hits"] > prev["hits"], (
            "the return frame served fragments from the cache"
        )

        # ── (G) damage region ───────────────────────────────────────
        moved = paint_after(tf, lambda: tf.scroll(SCROLL_TAG, to=(0, 640)))
        dmg = moved.get("last_damage_region")
        assert dmg is not None, "a frame that missed publishes its damage"
        for field in ("x", "y", "w", "h"):
            assert field in dmg, f"damage region carries `{field}`"
            assert isinstance(dmg[field], int), f"damage `{field}` is an int"
        assert dmg["w"] > 0 and dmg["h"] > 0, "the damage region has extent"
        # The number letting the cache into scrolled subtrees would otherwise
        # have wrecked. A virtual list's content container is as tall as the
        # data set: 10,000 rows at 32px = 320,000px, which is what this field
        # reported before the walk carried the clip — inside a 460px window.
        # A region larger than the surface is not a bound, it is noise.
        assert dmg["h"] <= WIN[1] and dmg["w"] <= WIN[0], (
            f"damage stays inside the {WIN[0]}x{WIN[1]} surface, got "
            f"{dmg['w']}x{dmg['h']} — an unclipped scrolled container reports "
            f"its whole content extent (320,000px tall for this binding)"
        )
        still = paint_after(tf, lambda: None)
        assert still.get("last_damage_region") is None, (
            "the next unchanged frame is a pure hit and publishes no damage"
        )
        assert_eq(
            still["misses"],
            moved["misses"],
            "an unchanged frame after a scroll encodes nothing new",
        )


run_demo("r1520_scrolled_paint_cache", body)
