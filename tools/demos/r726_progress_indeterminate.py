#!/usr/bin/env python3
"""R726 §5.28 §5.38 §5.40 indeterminate ProgressBar — looping sweep + scene/tick.

R726 widens the R718 ProgressBar with an indeterminate ("busy, completion
unknown") mode: `intervene("/external/indeterminate", true)` swaps the fraction
track for a looping accent segment driven by the §5.28 `IndeterminateSweep`
repeating Tickable (the caret_blink / SnackbarTimer sibling — but repeating, so
`is_at_rest` is always false while active). The a11y node omits `aria-valuenow`
(WAI-ARIA "unknown progress"). The sweep advances on the animation driver, so
the R724 `scene/tick` RPC drives it.

Verification:
  - boot: determinate (40 %), indeterminate flag false, no moving indicator;
  - intervene indeterminate=true: the `progress_indicator` segment appears,
    the readout switches to "Working…", `/external/indeterminate` reads true;
  - scene/tick advances the sweep: the indicator's x travels rightward and
    stays within the track bounds (the looping animation runs);
  - intervene indeterminate=false: determinate track returns, fraction
    preserved (40 %), indicator gone.

The exact sweep phase is real-frame-tick-confounded (a looping animation never
settles, unlike R724's theme-fade), so this asserts bounded forward movement +
the mode switch rather than an exact phase; scene/tick is the control surface.

Run from the workspace root:
    cargo build -p hello-progress --release
    python3 tools/demos/r726_progress_indeterminate.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (360, 160)
TRACK = "progress"
STATUS = "progress_status"
INDICATOR = "progress_indicator"
SEG_W = 72
TRACK_W = 240


def status_text(tf) -> str:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, STATUS)
    assert node is not None, "readout present"
    return node.get("content") or ""


def indicator_x(tf):
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, INDICATOR)
    return None if node is None else node["rect"]["x"]


def body() -> None:
    with RpcSubprocess("hello-progress", boot_grace=1.5) as tf:
        # ── boot: determinate, no sweep ──────────────────────────────
        assert_eq(tf.query("/external/indeterminate"), False, "boot is determinate")
        assert indicator_x(tf) is None, "no moving indicator while determinate"
        assert_eq(status_text(tf), "40%", "boot percent readout")

        # ── switch to indeterminate ──────────────────────────────────
        tf.intervene("/external/indeterminate", True)
        assert_eq(tf.query("/external/indeterminate"), True, "indeterminate set")
        assert_eq(status_text(tf), "Working\u2026", "busy readout")
        x0 = indicator_x(tf)
        assert x0 is not None, "moving indicator present while indeterminate"

        # Track absolute origin + bounds for the segment travel.
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        track = find_by_tag(snap, TRACK)
        assert track is not None, "track present"
        track_x = track["rect"]["x"]
        assert track_x <= x0 <= track_x + (TRACK_W - SEG_W), "indicator within track bounds"

        # ── scene/tick advances the sweep rightward ──────────────────
        tf.tick(0.45)  # quarter of the 1.8 s period
        x1 = indicator_x(tf)
        assert x1 is not None, "indicator still present after tick"
        assert x1 > x0, f"sweep advanced rightward ({x0} -> {x1})"
        assert x1 <= track_x + (TRACK_W - SEG_W), "indicator stays within track bounds"

        # ── back to determinate: fraction preserved, sweep gone ──────
        tf.intervene("/external/indeterminate", False)
        assert_eq(tf.query("/external/indeterminate"), False, "back to determinate")
        assert indicator_x(tf) is None, "indicator gone when determinate"
        assert_eq(status_text(tf), "40%", "fraction preserved across the round-trip")
        # The determinate value channel still works.
        assert abs(float(tf.query("/external/value")) - 0.40) < 1e-4, "value intact"


if __name__ == "__main__":
    sys.exit(run_demo("R726 indeterminate ProgressBar", body))
