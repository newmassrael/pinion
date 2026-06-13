#!/usr/bin/env python3
"""R919 §5.38 §5.27 — property-grid reset-to-default (the Details "reset arrow").

Drives `hello-property-grid` via JSON-RPC. The Details-panel inspector gains the
pervasive DCC affordance: a property changed from its class default shows a
**reset arrow**, and clicking it (or the RPC / keyboard) restores the default.
The baseline is the frozen `default_properties()`; "modified" is a value differing
from it (compared by `CellValue::sort_cmp`, the R900 NaN-safe total order), and
`reset` writes the baseline back through the SAME `set_value` funnel a keyboard
commit / RPC `intervene value.<i>` uses — one mutation path.

Witness (§2 #7 scene-as-data): the modified state is observable through
`/property_grid/external/{modified.<i>, any_modified}`, and the reset arrow's
presence in the paint scene (`property_grid#reset<source>`) tracks it exactly
(the paint==a11y one-gate). Row 4 ("Layer") is an `Int` defaulting to 3; row 6
("Pos X") is a `Float` defaulting to 12.5.

  (A) boot — nothing modified; no reset arrows.
  (B) edit a value — it reads modified; its reset arrow paints.
  (C) reset over RPC — the default returns; the arrow disappears.
  (D) reset by CLICKING the arrow — the same funnel.
  (E) reset_all — every modified property restored, count reported.
  (F) reset of an already-default row is a no-op; out-of-range reads honestly.

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-property-grid"
GRID = "property_grid"
VIEWPORT = (460, 820)


def gq(tf, slot):
    return tf.query(f"/{GRID}/external/{slot}")


def arrow_painted(tf, source: int) -> bool:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, f"{GRID}#reset{source}") is not None


def edit_int(tf, source: int, value: int) -> None:
    tf.intervene(f"/{GRID}/external/value.{source}", value)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — clean ────────────────────────────────────────
        assert_eq(gq(tf, "any_modified"), False, "boot: nothing modified")
        assert_eq(gq(tf, "modified.4"), False, "Layer is at its default")
        assert_eq(gq(tf, "modified.6"), False, "Pos X is at its default")
        assert_eq(gq(tf, "value.4"), 3, "Layer default is 3")
        assert not arrow_painted(tf, 4), "no reset arrow on a clean row"

        # ── (B) edit a value -> modified + reset arrow paints ───────
        edit_int(tf, 4, 17)
        assert_eq(gq(tf, "value.4"), 17, "the edit applied")
        assert_eq(gq(tf, "modified.4"), True, "an edited value reads modified")
        assert_eq(gq(tf, "any_modified"), True, "the grid is now dirty")
        assert_eq(gq(tf, "modified.6"), False, "an untouched row stays clean")
        wait_until(lambda: arrow_painted(tf, 4), timeout=4.0, interval=0.03,
                   desc="the modified row paints its reset arrow")
        assert not arrow_painted(tf, 6), "a clean row paints no arrow"

        # ── (C) reset over RPC -> default returns, arrow disappears ──
        assert_eq(tf.invoke(f"/{GRID}/external/reset", 4), True, "reset changed the modified row")
        assert_eq(gq(tf, "value.4"), 3, "reset restored the default")
        assert_eq(gq(tf, "modified.4"), False, "the row reads clean again")
        assert_eq(gq(tf, "any_modified"), False, "the grid is clean again")
        wait_until(lambda: not arrow_painted(tf, 4), timeout=4.0, interval=0.03,
                   desc="the reset arrow disappears once default")

        # ── (D) reset by CLICKING the arrow (the same funnel) ───────
        edit_int(tf, 4, 99)
        wait_until(lambda: arrow_painted(tf, 4), timeout=4.0, interval=0.03,
                   desc="the arrow returns after a fresh edit")
        tf.click(path=f"{GRID}#reset4")
        wait_until(lambda: gq(tf, "value.4") == 3, timeout=4.0, interval=0.03,
                   desc="clicking the arrow reset the value")
        assert_eq(gq(tf, "modified.4"), False, "the arrow click cleared modified")
        wait_until(lambda: not arrow_painted(tf, 4), timeout=4.0, interval=0.03,
                   desc="the arrow disappears after the click reset")

        # ── (E) reset_all restores every modified property ──────────
        edit_int(tf, 4, 50)
        tf.intervene(f"/{GRID}/external/value.6", 88.0)  # Pos X float
        assert_eq(gq(tf, "modified.4"), True, "Layer modified")
        assert_eq(gq(tf, "modified.6"), True, "Pos X modified")
        assert_eq(tf.invoke(f"/{GRID}/external/reset_all", None), 2, "reset_all reports the count reset")
        assert_eq(gq(tf, "value.4"), 3, "Layer restored")
        assert_eq(gq(tf, "value.6"), 12.5, "Pos X restored")
        assert_eq(gq(tf, "any_modified"), False, "the grid is clean after reset_all")

        # ── (F) no-op + honest out-of-range ─────────────────────────
        assert_eq(tf.invoke(f"/{GRID}/external/reset", 4), False, "reset of an already-default row is a no-op")
        assert_eq(tf.invoke(f"/{GRID}/external/reset_all", None), 0, "reset_all on a clean grid resets nothing")
        try:
            gq(tf, "modified.99")
            raise AssertionError("an out-of-range modified read should be an UnknownPath error")
        except RpcError:
            pass


if __name__ == "__main__":
    sys.exit(run_demo("R919 §5.38 §5.27 — property-grid reset-to-default", body))
