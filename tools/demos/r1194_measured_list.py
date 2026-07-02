#!/usr/bin/env python3
"""R1194 §5.27 — measured variable-height list virtualization E2E.

Drives the `hello-measured-list` binding via JSON-RPC. Consumer of
`pinion_widget_paint::virtual_list::view_measured_list` — the *measured*
peer of R745's `view_variable_virtual_list`. Where R745 takes a caller-
supplied prefix-sum table of KNOWN heights, this list does not know its row
heights up front: each row is a stack of `lines(i)` fixed-height strips, so
its natural height is `lines(i)·STRIP_H`, but that height is discovered by
laying the row out (the layout-pass measurement round-trip) — never handed
to the windowing. The estimate `EST=48` misses every tier (22/44/66/88/110),
so the total content height refines from the all-estimate baseline
(N·EST=5760) toward the exact sum (exact_total=7920) as rows are measured.

The witness is scene-as-data + introspection (§2 #7), no pixels:

  (A) boot window — only a small window of the 120 rows exists; row 0 at
      the top; deep rows absent.
  (B) the round-trip — each rendered `measured-row:<i>` slot's laid-out
      height equals its MODELED height (the harvest read the real content
      height, not the estimate); adjacent slot tops differ by exactly the
      upper row's measured height (the refined offsets drive geometry).
  (C) introspection — the primary External reports item_count / estimated /
      exact_total, and at boot only the visible window is measured
      (measured_count > 0, is_fully_measured False, a deep row's
      measured_height is Null).
  (D)+(E) refinement + convergence — scrolling the whole list monotonically
      grows measured_count; once every row has been windowed,
      is_fully_measured is True and total_height has converged to
      exact_total (proving the estimate really was refined away).
  (F) offset coherence — scroll clamps to exact_total − viewport at the
      bottom (the refined sizer drove max_y), the last row is reachable,
      and scrolling back to the top restores the row-0 window.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

EXAMPLE = "hello-measured-list"
WIN = (360, 520)
N = 120
STRIP_H = 22
MAX_LINES = 5
EST = 48
VP_H = 330
OVERSCAN = 2
SCROLL_TAG = "mlist_scroll"
LIST_TAG = "mlist"
BAR_TAG = "mlist_scrollbar"


def model_height(i: int) -> int:
    """Python mirror of the row's natural height — never given to windowing,
    only used to check what the harvest measured."""
    return (1 + i % MAX_LINES) * STRIP_H


EXACT_TOTAL = sum(model_height(i) for i in range(N))  # 7920
BASELINE = N * EST  # 5760


def present_rows(snap) -> set[int]:
    out: set[int] = set()
    for tag in abs_rects_of(snap):
        if tag.startswith("measured-row:"):
            out.add(int(tag.split(":", 1)[1]))
    return out


def slot_rect(snap, i: int):
    return abs_rects_of(snap)[f"measured-row:{i}"]


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot window: a small window of 120 rows ──────────────
        rects = abs_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert SCROLL_TAG in rects, "scroll container present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")

        rows = present_rows(snap)
        assert 0 in rows, "row 0 rendered at the top"
        assert len(rows) < 40, f"virtualized: small window, got {len(rows)} of {N}"
        assert (N - 1) not in rows, "the last row is NOT rendered at the top"

        # ── (C) introspection: the measurement state at boot ─────────
        assert_eq(tf.query("/external/item_count"), N, "item_count")
        assert_eq(tf.query("/external/estimated"), EST, "estimate")
        assert_eq(tf.query("/external/exact_total"), EXACT_TOTAL, "exact total")
        assert BASELINE != EXACT_TOTAL, "estimate is offset from the exact total"
        boot_measured = tf.query("/external/measured_count")
        assert boot_measured >= len(rows), \
            f"boot window measured ({boot_measured} >= {len(rows)})"
        assert_eq(tf.query("/external/is_fully_measured"), False,
                  "not fully measured at boot")
        assert_eq(tf.query(f"/external/measured_height.{N - 1}"), None,
                  "a never-windowed deep row reports Null, not the estimate")

        # ── (B) the round-trip: slot heights == modeled heights ──────
        # Each rendered slot's laid-out height is the row's real content
        # height — proof the harvest measured the content, not the estimate.
        for i in sorted(rows):
            assert_eq(slot_rect(snap, i)[3], model_height(i),
                      f"row {i} laid-out height == modeled {model_height(i)}")
            assert_eq(tf.query(f"/external/model_height.{i}"), model_height(i),
                      f"row {i} model_height query")
            assert_eq(tf.query(f"/external/measured_height.{i}"), model_height(i),
                      f"row {i} harvested height == model (round-trip)")
        # Heights genuinely vary (not a uniform pitch in disguise).
        seen = {slot_rect(snap, i)[3] for i in rows}
        assert len(seen) >= 3, f"rows show multiple distinct heights, got {sorted(seen)}"
        # Adjacent slot tops differ by exactly the upper row's measured height
        # (the refined offset table drives geometry).
        for i in sorted(rows)[:-1]:
            if i + 1 in rows:
                dy = slot_rect(snap, i + 1)[1] - slot_rect(snap, i)[1]
                assert_eq(dy, model_height(i),
                          f"row {i}->{i+1} top delta == row {i} measured height")

        # ── (D)+(E) refinement + convergence: scroll the whole list ──
        # A settled measured list refines each row as it is windowed. Walk
        # the list top-to-bottom; measured_count is monotonic and, once every
        # row has been seen, the total converges to the exact sum.
        prev_measured = boot_measured
        for _ in range(80):
            if tf.query("/external/is_fully_measured"):
                break
            before = scroll_offset(tf.snapshot(source="paint", viewport=WIN))
            tf.wheel(path=SCROLL_TAG, pixels=(0.0, 240.0))
            # Not fully measured yet ⇒ unmeasured rows remain below ⇒ not at
            # the bottom ⇒ the wheel advances the offset (robust wait, no sleep).
            snap = wait_snap(
                tf, lambda s, b=before: scroll_offset(s) > b, viewport=WIN,
                desc="wheel advanced the offset toward the bottom",
            )
            cur = tf.query("/external/measured_count")
            assert cur >= prev_measured, \
                f"measured_count is monotonic ({cur} >= {prev_measured})"
            prev_measured = cur

        assert_eq(tf.query("/external/is_fully_measured"), True,
                  "every row measured after a full traversal")
        assert_eq(tf.query("/external/measured_count"), N, "all N rows measured")
        assert_eq(tf.query("/external/total_height"), EXACT_TOTAL,
                  "total_height converged from the estimate to the exact sum")

        # ── (F) offset coherence against the refined table ───────────
        tf.scroll(SCROLL_TAG, to=(0, 10**9))  # past the end → clamps
        max_off = EXACT_TOTAL - VP_H  # 7920 - 330 = 7590
        snap = wait_snap(
            tf, lambda s: scroll_offset(s) == max_off, viewport=WIN,
            desc="offset clamps to exact_total - viewport (refined sizer drove max_y)",
        )
        assert (N - 1) in present_rows(snap), "the last row is reachable at the bottom"

        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf, lambda s: scroll_offset(s) == 0, viewport=WIN,
            desc="scrolled back to the top",
        )
        top_rows = present_rows(snap)
        assert 0 in top_rows, "row 0 rendered at the top again"
        assert len(top_rows) < 40, "top window stays small"
        # Now every row is measured, so the window geometry is exact.
        for i in sorted(top_rows):
            assert_eq(slot_rect(snap, i)[3], model_height(i),
                      f"row {i} height still == model after full measurement")


if __name__ == "__main__":
    sys.exit(run_demo("R1194 §5.27 — measured variable-height virtualization", body))
