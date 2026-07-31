#!/usr/bin/env python3
"""R1521 §5.36 §5.7 §2 #2 — the shape cache grows into its working set.

The §5.36 `LayoutCache` had a fixed LRU capacity of 256, and a UI frame
walks straight into the one failure mode a fixed LRU has. Painting is a
CYCLIC access pattern — every frame visits the same labels in the same
order — and LRU is pathological on a cycle longer than its capacity: each
entry is evicted by the one requested just before it comes round again.
The hit rate is not degraded but ZERO, and it is zero on every subsequent
frame, forever.

Measured in the paint walk before the fix (release, this machine):

    text leaves     steady-state paint      shapes per frame
    256             0.53 ms                 0
    300             5.35 ms                 300
    1200            27.4 ms                 1200

A 17% increase in content multiplied the per-frame cost by TEN, and 1,200
leaves — a 30-column data grid with 40 visible rows — cost 1.6x the whole
60fps budget on shaping alone. That is a cliff, not a cost curve.

R1521 grows the capacity when the cache catches PROOF that it was too
small: a miss on a key this cache itself evicted (the ghost list of 2Q /
ARC). A key that comes back after eviction witnesses a working set that
did not fit; a key that never comes back is a scan, and a scan must not
grow anything. Bounded by `MAX_CAPACITY` (8,192 entries, ~26 MB at the
measured ~3.1 KB each).

## What this demo drives, and why this binding

Scrolling a large list down and back up, repeatedly. That is not a
contrived pattern — it is what a log viewer, an asset browser and a scene
outliner spend their time doing, and it is exactly a cyclic working set:
the rows visited on the way down are visited again on the way up, and
again on the next sweep. `hello-million-row` and `hello-virtual-list` are
the Model/View-at-scale bindings, and both cross 256 within one sweep.

Worth stating plainly: NO shipped binding exceeds 256 on a static boot
frame — the widest, `hello-grid-frozen-col`, shapes 198. The cliff is
reached by USE, not by opening the app, which is why a boot-frame census
would have reported everything healthy.

Numbers below are measured on these bindings, both ways — the "before"
column is a real run with the capacity pinned at 256, not an estimate.
Cumulative `shapes` after each sweep:

                              pinned 256      R1521
    million-row,  sweep 1          691           684
    million-row,  sweep 2         1087           684
    million-row,  sweep 3         1483           684
    million-row,  sweep 4         1879           684
    virtual-list, sweep 1          524           524
    virtual-list, sweep 2          798           524
    virtual-list, sweep 3         1072           524
    virtual-list, sweep 4         1346           524

+396 and +274 shapes per sweep, forever, against +0. Note that
`virtual-list` sweep 1 is IDENTICAL in both columns — the first pass over
cold content costs the same either way, and everything the round changed
is in the steady state, which is where an application actually lives.

## Verification scope (>= 30 assertions, sections A-F)

  (A) `scene/text_cache_stats` typed surface — every documented field
      present with the documented type; `at_ceiling` derived, not echoed.
  (B) It is a DIFFERENT cache from `scene/cache_stats`. The fragment
      cache reports a healthy hit rate while the shaper thrashes
      underneath it, which is why the shape cache needed its own wire.
  (C) Boot census — a quiet frame shapes its labels once and stops.
  (D) A cyclic sweep past the capacity grows the cache, and `growths`
      records it. Pinned at 256 this stays 0 forever.
  (E) STEADY STATE — the assertion the whole round is about: once grown,
      further identical sweeps shape NOTHING. Pinned at 256 each sweep
      re-shapes ~150 strings.
  (F) Growth is proportionate and bounded — the cache settles near its
      working set, not at the ceiling, and never passes `max_capacity`.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

# Two Model/View-at-scale bindings, both of which cross 256 by scrolling.
# `hello-million-row` carries the wider row (more text per leaf) and grows
# faster; `hello-virtual-list` confirms the behaviour is the cache's and not
# one binding's.
TARGETS = (
    ("hello-million-row", "million_scroll", 396),
    ("hello-virtual-list", "vlist_scroll", 274),
)

# One sweep: down through the data set and back up. The return leg is what
# makes the working set CYCLIC rather than a scan — a scan must not grow the
# cache, and (per the unit tests) does not.
#
# The span is chosen from measurement, not taste: at 4,000 px one sweep leaves
# 241 entries, just UNDER the old 256, so the precondition for the defect is
# not met and the demo would pass without ever exercising it. At 6,000 px both
# bindings clear 256 within one sweep (467 and 390 entries).
SWEEP_STOPS = tuple(range(0, 6000, 250))

# Documented wire fields and their JSON types.
FIELDS = (
    ("shapes", int),
    ("entries", int),
    ("capacity", int),
    ("max_capacity", int),
    ("growths", int),
    ("font_scans", int),
    ("at_ceiling", bool),
)


def sweep(tf: RpcSubprocess, tag: str) -> None:
    """Scroll down through the data set and back up.

    `scene/scroll` mutates the attached `ScrollState` without arming a
    redraw, so the `tick` is what turns each mutation into a painted frame
    (R1520's rig lesson — without it the paint never happens and the
    working set never forms).
    """
    for y in SWEEP_STOPS:
        tf.scroll(tag, to=(0, y))
        tf.tick(0.016)
    for y in reversed(SWEEP_STOPS):
        tf.scroll(tag, to=(0, y))
        tf.tick(0.016)


def drive(tf: RpcSubprocess, example: str, tag: str, pinned_growth: int) -> None:
    # ── (A) the typed surface ───────────────────────────────────────
    st = tf.text_cache_stats()
    for field, kind in FIELDS:
        assert field in st, f"{example}: text_cache_stats publishes `{field}`"
        assert isinstance(st[field], kind), (
            f"{example}: `{field}` is {kind.__name__}, got {type(st[field]).__name__}"
        )
    assert st["max_capacity"] >= st["capacity"], (
        f"{example}: a ceiling below the capacity would describe no cache"
    )
    assert_eq(
        st["at_ceiling"],
        st["capacity"] >= st["max_capacity"],
        f"{example}: at_ceiling is derived from the pair, not carried",
    )
    assert st["font_scans"] <= 1, (
        f"{example}: R1447 invariant — the platform font scan runs at most "
        f"once per cache, got {st['font_scans']}"
    )

    # ── (B) it is a different cache from the fragment cache ─────────
    frag = tf.cache_stats()
    assert "hits" in frag and "entries" in frag, f"{example}: fragment cache reachable"
    assert "hits" not in st, (
        f"{example}: the shape cache reports MISSES (`shapes`), not a hit "
        f"count — conflating the two surfaces is what hid this defect"
    )
    assert "shapes" not in frag, f"{example}: and the fragment cache does not report shapes"

    # ── (C) boot census: a quiet frame shapes once ──────────────────
    boot = st["shapes"]
    assert boot > 0, f"{example}: the boot paint shaped its labels"
    tf.tick(0.016)
    tf.tick(0.016)
    quiet = tf.text_cache_stats()
    assert_eq(
        quiet["shapes"],
        boot,
        f"{example}: idle frames re-shape nothing — the boot set is warm",
    )
    assert_eq(quiet["growths"], 0, f"{example}: and a fitting working set never grew")

    # ── (D) a cyclic sweep past the capacity grows the cache ────────
    sweep(tf, tag)
    first = tf.text_cache_stats()
    assert first["shapes"] > boot, f"{example}: the sweep brought new rows into view"
    # The precondition is stated over SHAPES, not over `entries`, and the
    # difference is the difference between a gate and a tautology. `entries`
    # cannot exceed the capacity by construction, so asserting `entries > 256`
    # is unsatisfiable on the very build this demo exists to discriminate
    # against — it would fail there for the wrong reason and hide whether the
    # real claims below can be reached at all. Distinct strings shaped in one
    # sweep is capacity-independent: 650 here, 494 on `hello-virtual-list`,
    # the same on both builds.
    distinct = first["shapes"] - boot
    assert distinct > 256, (
        f"{example}: one sweep shaped {distinct} distinct strings, so the "
        f"working set exceeds the old fixed capacity of 256 — that is the "
        f"precondition for the defect, and without it this demo proves nothing"
    )
    assert first["growths"] >= 1, (
        f"{example}: the cache caught a key it had evicted coming back, and "
        f"grew; pinned at 256 this stays 0 forever"
    )
    assert first["capacity"] > 256, f"{example}: and the capacity actually moved"

    # ── (E) steady state — the point of the round ───────────────────
    settled = first["shapes"]
    for i in range(2):
        sweep(tf, tag)
        now = tf.text_cache_stats()
        assert_eq(
            now["shapes"],
            settled,
            f"{example}: sweep {i + 2} re-shapes NOTHING; with the capacity "
            f"pinned at 256 it re-shapes ~{pinned_growth} strings, and does "
            f"so on every sweep for the life of the process",
        )
        assert_eq(
            now["growths"],
            first["growths"],
            f"{example}: sweep {i + 2} needed no further growth — the working "
            f"set fits now, so the evidence stops arriving",
        )

    # ── (F) proportionate and bounded ───────────────────────────────
    final = tf.text_cache_stats()
    assert final["capacity"] <= final["max_capacity"], (
        f"{example}: growth never passes the stated ceiling"
    )
    assert final["capacity"] >= final["entries"], (
        f"{example}: the capacity covers what is held"
    )
    assert final["capacity"] < 8192, (
        f"{example}: a {final['entries']}-entry working set settles NEAR its "
        f"size, not at the 8192 ceiling — a growth rule that spent one "
        f"evicted key at a time lands at 8192 here and passes every other "
        f"assertion in this demo"
    )
    assert not final["at_ceiling"], f"{example}: so the cache still has room"
    assert_eq(
        final["font_scans"],
        quiet["font_scans"],
        f"{example}: growing the cache does not re-enumerate the platform fonts",
    )


def body() -> None:
    for example, tag, pinned_growth in TARGETS:
        with RpcSubprocess(example, boot_grace=1.5) as tf:
            for _ in range(3):
                tf.tick(0.016)
            drive(tf, example, tag, pinned_growth)


if __name__ == "__main__":
    run_demo("r1521_shape_cache_working_set", body)
