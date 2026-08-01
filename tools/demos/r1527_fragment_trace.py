#!/usr/bin/env python3
"""R1527 §5.16 §2 #7 — the paint cache's mark phase gets a trace step.

R682 built the §5.16 fragment cache with a mark-and-sweep eviction and
wrote down the bound it was aiming for: *"memory bounds itself to the set
of cacheable Containers actually painted in the most recent frame."* The
implementation then used a proxy for that set — the hashes the walker
**consulted** — and painted and consulted are the same set only while the
cache is missing. The instant a container hits, the walk returns without
descending, so every fragment underneath it is painted (its pixels are in
the replayed fragment) and never consulted. The sweep read that silence as
death.

So the divergence was worst exactly where the cache worked best, and it
was not a slow leak but a collapse in a single frame. R1520 registered it
as a debt with three candidate fixes — grace frames, an LRU with a byte
budget, or descending past a hit to mark — and each gives up something
R682 chose deliberately (the absent cap, or the short-circuit that IS the
cache's benefit). None was needed. A mark-and-sweep collector that marks
its roots and never follows an edge collects live objects; this one had no
edge to follow. Containment is that edge, and tracing it makes the code
compute the bound its own doc always stated.

## What this demo drives, and why this binding

`hello-grid-nav` is a 10,000-row keyboard-navigable data grid — the shape
a scene outliner, an asset browser and a log view all have, and the shape
where the cost lands: ArrowDown moves the selection by ONE row, so one row
strip re-paints and every other fragment on screen is unchanged.

Measured on this binding, both ways — the "before" column is a real run
with the trace step removed, not an estimate:

                                      pre-R1527    R1527
    entries after one idle frame           1          83
    ArrowDown: fragments reused            1          20
    ArrowDown: fragments re-encoded       83          10

Every arrow key re-encoded the whole grid. Not because anything about it
had changed, but because the idle frame before it had thrown the grid
away through the single root that replayed it.

The same collapse in a unit-level measurement, 1,200 rows at one cacheable
container each: changing ONE row cost 17.1 ms at ZERO hits, against 1.4 ms
at 1,199 hits once the trace lands — 59-103% of a 60fps budget spent on a
1/1200 delta.

## Verification scope (>= 30 assertions, sections A-G)

  (A) `scene/cache_stats` typed surface — every documented field present
      with the documented type, `hit_rate` consistent with hits/misses.
  (B) Boot census — one fragment per cacheable container.
  (C) An idle frame keeps what its root replayed. The headline: `entries`
      holds at the boot census across repeated idle frames, while hits
      advance by exactly one (the root) and misses do not move at all.
      Pre-R1527 this collapsed to 1 on the first idle frame.
  (D) One selection step reuses the rest of the grid — strictly more
      fragments replayed than encoded, on a frame that changed one row,
      and that frame publishes a damage region bounded by the surface.
      Pre-R1527: 1 hit against 83 misses, every keystroke.
  (E) Sustained stepping — the reuse is steady state, not a one-frame
      artifact, and `entries` does not grow while it happens.
  (F) NEGATIVE CONTROL — a jump across the dataset (`End`, row 9,999)
      must re-encode nearly everything. Retention that survived this
      would be staleness, not reuse, and (G) is what would catch it in
      pixels; this catches it in the counters.
  (G) Pixel witness — leaving a selection and returning to it paints a
      framebuffer byte-identical to the first visit, with the second
      visit served largely from retained fragments. A fragment kept alive
      by the trace but stale in content differs here and nowhere else.
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
    read_png_rgba8,
    run_demo,
    wait_paint_beyond,
    wait_query,
    wait_until,
)

EXAMPLE = "hello-grid-nav"
WIN = (400, 480)
TABLE_TAG = "vtbl"
# The binding's boot census, measured. Asserted as a floor rather than an
# equality: it tracks the viewport's row window, and this demo's claim is
# about what SURVIVES a frame, not about how many rows fit in one.
MIN_BOOT_ENTRIES = 40


def stats(tf: RpcSubprocess) -> dict:
    return tf.cache_stats()


def paint_after(tf: RpcSubprocess, action, tick: bool = True) -> dict:
    """Run `action`, land exactly one frame, and return the fresh stats.

    `paint_count` is the only counter `AppShell::render_window` advances,
    so gating on it is what makes "one frame" a fact rather than a sleep.

    `tick=False` for an action that arms its own redraw. A key event does;
    a programmatic `scene/scroll` does not (it mutates the attached
    `ScrollState` with nothing to wake the loop), which is why the tick is
    the default. Ticking anyway is not harmless: measured on this binding
    it lands exactly TWO frames per keystroke, deterministically, and the
    second is a pure hit that overwrites every per-frame observable —
    which is how `last_damage_region` came back `None` on a frame that had
    just re-encoded, and nearly cost this demo the assertion below.
    """
    before = int(stats(tf)["paint_count"])
    action()
    if tick:
        tf.tick(0.016)
    wait_paint_beyond(tf, before)
    return stats(tf)


def capture(tf: RpcSubprocess, name: str) -> Png:
    out = Path(tempfile.mkdtemp(prefix="pinion-r1527-")) / f"{name}.png"
    res = tf.request("scene/screenshot", {"path": "", "out_path": str(out)})
    assert res.result, f"{name}: screenshot returned no result"
    assert_eq((res.result["width"], res.result["height"]), WIN, f"{name} extent")
    assert out.exists(), f"{name}: no PNG at {out}"
    return read_png_rgba8(out)


def step_to(tf: RpcSubprocess, key: str, target: int) -> dict:
    """One keyboard step, landing exactly one frame, and settle on it."""
    st = paint_after(tf, lambda: tf.key(path=TABLE_TAG, name=key), tick=False)
    wait_query(tf, "/external/selected", target, desc=f"{key} selects row {target}")
    return st


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
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
        assert abs(st["hit_rate"] - st["hits"] / total) < 1e-9, (
            "hit_rate is hits/(hits+misses), computed framework-side"
        )

        # ── (B) boot census: one fragment per cacheable container ────
        assert st["entries"] >= MIN_BOOT_ENTRIES, (
            f"the grid offers a fragment per cacheable container, got "
            f"{st['entries']}"
        )
        assert_eq(st["hits"], 0, "nothing to hit on the first paint")
        assert_eq(
            st["misses"], st["entries"],
            "every first-paint miss installed exactly one fragment",
        )
        boot_entries = st["entries"]

        # ── (C) THE HEADLINE: an idle frame keeps what its root replayed
        idle_prev = st
        for i in range(3):
            now = paint_after(tf, lambda: None)
            assert_eq(
                now["hits"] - idle_prev["hits"], 1,
                f"idle paint {i}: exactly one hit — the root answers alone",
            )
            assert_eq(
                now["misses"] - idle_prev["misses"], 0,
                f"idle paint {i}: an unchanged scene encodes nothing",
            )
            assert_eq(
                now["entries"], boot_entries,
                f"idle paint {i}: the fragments that root replayed are "
                f"painted, so they survive the sweep (pre-R1527: 1)",
            )
            assert now.get("last_damage_region") is None, (
                f"idle paint {i}: a 100% hit publishes no damage"
            )
            idle_prev = now

        # ── (D) one selection step reuses the rest of the grid ───────
        tf.request("focus/set", {"tag": TABLE_TAG})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == TABLE_TAG,
            desc="grid owns focus",
        )
        # The first ArrowDown selects row 0 from nothing selected, which
        # restyles the strip that gains the selection only. Step twice
        # more so the measured frame is a true one-row *move*: one strip
        # loses the selection, one gains it.
        step_to(tf, "ArrowDown", 0)
        step_to(tf, "ArrowDown", 1)
        before = stats(tf)
        moved = step_to(tf, "ArrowDown", 2)
        gained = moved["hits"] - before["hits"]
        encoded = moved["misses"] - before["misses"]
        assert gained > 0, (
            f"a one-row selection move replays fragments it did not "
            f"re-encode, got {gained} (pre-R1527: 1, and that one was the root)"
        )
        assert gained > encoded, (
            f"and replays MORE than it encodes: {gained} reused vs "
            f"{encoded} encoded (pre-R1527: 1 vs 83)"
        )
        assert encoded > 0, (
            "the row that changed is really re-encoded — a frame that "
            "encoded nothing would mean the selection never reached paint"
        )
        assert_eq(
            moved["entries"], boot_entries,
            "the window still holds one fragment per cacheable container",
        )
        dmg = moved.get("last_damage_region")
        assert dmg is not None, (
            "a frame that re-encoded publishes where its pixels may differ"
        )
        for field in ("x", "y", "w", "h"):
            assert field in dmg, f"damage region carries `{field}`"
        assert dmg["w"] > 0 and dmg["h"] > 0, "the damage region has extent"
        assert dmg["w"] <= WIN[0] and dmg["h"] <= WIN[1], (
            f"damage stays inside the {WIN[0]}x{WIN[1]} surface, got "
            f"{dmg['w']}x{dmg['h']}"
        )

        # ── (E) the reuse is steady state, not one frame ─────────────
        run_hits = 0
        run_misses = 0
        prev = moved
        for target in range(3, 8):
            now = step_to(tf, "ArrowDown", target)
            h = now["hits"] - prev["hits"]
            m = now["misses"] - prev["misses"]
            assert h > m, (
                f"step to row {target} reuses more than it encodes, "
                f"got {h} vs {m}"
            )
            assert_eq(
                now["entries"], boot_entries,
                f"step to row {target}: the live set neither collapses nor grows",
            )
            run_hits += h
            run_misses += m
            prev = now
        ratio = run_hits / (run_hits + run_misses)
        assert ratio > 0.6, (
            f"across five keystrokes the grid replays {ratio:.0%} of the "
            f"fragments it paints (pre-R1527: 1/84 = 1%)"
        )

        # ── (F) NEGATIVE CONTROL: a real change must really re-encode ─
        # `End` selects row 9,999 and scrolls there, so the entire visible
        # window is content the cache has never seen. Reuse here would not
        # be reuse; it would be a stale fragment served for new content.
        jumped = paint_after(tf, lambda: tf.key(path=TABLE_TAG, name="End"), tick=False)
        wait_query(tf, "/external/selected", 9999, desc="End selects the last row")
        jump_hits = jumped["hits"] - prev["hits"]
        jump_misses = jumped["misses"] - prev["misses"]
        assert jump_misses > jump_hits, (
            f"a jump across the dataset encodes what it shows: {jump_misses} "
            f"encoded vs {jump_hits} reused — retention that survived this "
            f"would be staleness"
        )
        assert jump_misses > run_misses / 5, (
            "and it costs far more than a one-row step, which is the "
            "whole point of the step being cheap"
        )
        assert_eq(
            jumped["entries"], boot_entries,
            "content that left the window is collected — reachability did "
            "not become retention",
        )

        # ── (G) pixel witness: a retained fragment is not a stale one ─
        tf.key(path=TABLE_TAG, name="Home")
        wait_query(tf, "/external/selected", 0, desc="Home returns to the top")
        paint_after(tf, lambda: None)
        step_to(tf, "ArrowDown", 1)
        step_to(tf, "ArrowDown", 2)
        paint_after(tf, lambda: None)
        first_visit = capture(tf, "sel2_first")

        # Leave and come back. The return trip is served largely from
        # fragments the trace kept alive across the intervening frames.
        for target in (3, 4, 5):
            step_to(tf, "ArrowDown", target)
        back = stats(tf)
        for target in (4, 3, 2):
            step_to(tf, "ArrowUp", target)
        returned = stats(tf)
        assert returned["hits"] > back["hits"], (
            "the return trip replayed fragments rather than encoding all"
        )
        paint_after(tf, lambda: None)
        second_visit = capture(tf, "sel2_second")

        assert_eq(
            (second_visit.width, second_visit.height),
            (first_visit.width, first_visit.height),
            "both captures are the same surface",
        )
        assert second_visit.pixels == first_visit.pixels, (
            "returning to a selection paints byte-identically to the first "
            "visit — a fragment the trace kept alive but that had gone "
            "stale would differ exactly here"
        )
        assert_eq(
            stats(tf)["entries"], boot_entries,
            "and the live set is still exactly the painted set",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1527 §5.16 — the mark phase has a trace step", body))
