#!/usr/bin/env python3
"""R1422 §5.35 §5.15 — a raw pointer stream synthesises a double-click.

The R1416/R1418 raw multi-button sink (`External::wants_raw_pointer_buttons`)
carried the button, both edges, the held modifiers, and the held set — but not
the ONE verb a real `QMouseEvent` stream still has: the double-click. Qt marks
the second press of a fast in-place repeat as `MouseButtonDblClick`; the DOM
carries the same count as `MouseEvent.detail`. R1422 gives the raw edge that
count as `RawPointerButton::click_count`, synthesised by the router — so a sink
reads a double-click without re-implementing the timing.

The synthesis lives in the ONE `deliver_raw_pointer_button` seam both the native
winit `MouseInput` path and the `scene/pointer_button` RPC cross
(`[[r47-class-incident-prevention]]`), reusing the SAME time + distance window as
the send-wire `DoubleClick` path so the two double-click rules cannot drift.

`scene/double_click` is the deterministic wire driver: R1416 already routes it
through `pointer_button_for_window`, so on a raw sink it delivers two down/up
cycles in ONE drain batch — the second press lands microseconds after the first,
inside the window, so it reads 2 every run (no wall-clock race). The router's
per-button double-click mark persists across the sink's `clear` (exactly as Qt's
cycle does), so each double-click phase is `arm_double`-d first: a FAR press
plants a position-mismatched mark, guaranteeing the double_click's FIRST down is
a fresh single (its SECOND down is then the true double). The negatives (a fresh
button, a would-be triple, a strayed press) must stay 1.

Run from the workspace root:
    cargo build -p hello-raw-pointer --release
    python3 tools/demos/r1422_double_click.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_snap,
)

WIN = (720, 440)
PANE = "pane"
EXT = "/external"

# Mirror of the binding's PANE_RECT = Rect::new(16, 48, WIN_W - 32, WIN_H - 104).
RECT_X, RECT_Y, RECT_W, RECT_H = 16, 48, WIN[0] - 32, WIN[1] - 104


def pane_at(fx: float, fy: float) -> tuple[float, float]:
    """The window pixel at fraction `(fx, fy)` across the pane rect."""
    return (RECT_X + fx * RECT_W, RECT_Y + fy * RECT_H)


CENTER = pane_at(0.5, 0.5)
# Far enough from CENTER (0.4 * 688 ≈ 275 px) to blow past the 5 px double-click
# distance tolerance — a press here can never pair with a CENTER press.
FAR = pane_at(0.9, 0.5)


def readout(snap) -> str:
    node = find_by_tag(snap, "raw.readout")
    return node.get("content", "") if node else ""


def clear(tf) -> None:
    tf.invoke(f"{EXT}/clear", None)
    wait_query(tf, f"{EXT}/report_count", 0, desc="log cleared")


def arm_double(tf) -> None:
    """Deterministically prime a fresh left double-click at CENTER.

    The router's per-button double-click mark survives the sink's `clear` (the
    Qt cycle is global per button, not per report log), so a prior CENTER press
    inside the window would make a `double_click`'s FIRST down read as a double.
    A FAR press (> 5 px away) plants a position-mismatched mark, so the next
    CENTER down can never pair with it — the double_click's first down is then a
    guaranteed single, and its second down is the true double.
    """
    tf.pointer_button("left", "down", at=FAR)
    tf.pointer_button("left", "up", at=FAR)
    clear(tf)


def body() -> None:
    with RpcSubprocess("hello-raw-pointer") as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, PANE) is not None,
            source="paint",
            viewport=WIN,
            desc="pane surface resolved",
        )

        # --- boot: nothing pressed, no click count ---
        assert_eq(tf.query(f"{EXT}/report_count"), 0, "boot: no reports")
        assert_eq(tf.query(f"{EXT}/last_clicks"), None, "boot: no click count")
        assert "press any mouse button" in readout(snap), "boot: the idle prompt"

        # --- a single click is click_count 1 on BOTH edges (Qt MouseButtonPress),
        #     and carries no double-click badge. The first press ever has no prior
        #     mark, so it is a clean single. ---
        tf.pointer_button("left", "down", at=CENTER)
        wait_query(tf, f"{EXT}/report_count", 1, desc="a first press is recorded")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 1, "a first press is click_count 1")
        assert_eq(tf.query(f"{EXT}/last_button"), "left", "the button is identified")
        assert_eq(tf.query(f"{EXT}/last_edge"), "down", "the edge is identified")
        tf.pointer_button("left", "up", at=CENTER)
        wait_query(tf, f"{EXT}/report_count", 2, desc="its release is recorded")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 1, "the release echoes click_count 1")
        snap = wait_snap(
            tf,
            lambda s: "left:up" in readout(s),
            source="paint",
            viewport=WIN,
            desc="the readout names the single click",
        )
        assert "×" not in readout(snap), "a single click carries no ×N badge"

        # --- THE DOUBLE-CLICK: two down/up cycles in one drain, so the second
        #     press reads 2 — the Qt MouseButtonDblClick the raw stream never
        #     expressed before R1422. The last edge (the second release) echoes
        #     the 2, which REQUIRES the second press to have been 2. ---
        arm_double(tf)
        tf.double_click(at=CENTER)
        wait_query(tf, f"{EXT}/report_count", 4, desc="the double's four edges drained")
        assert_eq(tf.query(f"{EXT}/last_button"), "left", "the double is a left button")
        assert_eq(tf.query(f"{EXT}/last_edge"), "up", "the last edge is the second release")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 2, "the double-click count reaches 2")
        snap = wait_snap(
            tf,
            lambda s: "×2" in readout(s),
            source="paint",
            viewport=WIN,
            desc="the readout badges the double-click ×2",
        )
        assert "×2" in readout(snap), "the readout shows the ×2 double-click badge"

        # --- NEGATIVE: the count is independent per button. A left double must not
        #     make a following RIGHT press read as a double — the tracker keys on
        #     the button, so a fresh button is always a first click. ---
        arm_double(tf)
        tf.double_click(at=CENTER)  # left double
        wait_query(tf, f"{EXT}/report_count", 4, desc="the left double drained")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 2, "the left double is 2")
        tf.pointer_button("right", "down", at=CENTER)  # a fresh button
        wait_query(tf, f"{EXT}/report_count", 5, desc="the right press drained")
        assert_eq(tf.query(f"{EXT}/last_button"), "right", "the changed button is right")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 1, "the right press is a fresh single")
        tf.pointer_button("right", "up", at=CENTER)

        # --- NEGATIVE: no rolling triple. After a double (count 2) a third press at
        #     the same spot resets to 1 — the send-wire DoubleClick's binary
        #     single/double rule on the raw axis. ---
        arm_double(tf)
        tf.double_click(at=CENTER)  # ends with the second press marked 2
        wait_query(tf, f"{EXT}/report_count", 4, desc="the double drained")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 2, "the double is 2 before the third press")
        tf.pointer_button("left", "down", at=CENTER)  # the third press
        wait_query(tf, f"{EXT}/report_count", 5, desc="the third press drained")
        assert_eq(tf.query(f"{EXT}/last_edge"), "down", "the last edge is the third press")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 1, "the third press starts a fresh cycle")
        tf.pointer_button("left", "up", at=CENTER)

        # --- NEGATIVE: a second press that strayed past the 5 px tolerance is not a
        #     double (the intentional-drag guard shared with the send wire). A
        #     CENTER press then a FAR press (~275 px) can never pair. ---
        arm_double(tf)
        tf.pointer_button("left", "down", at=CENTER)
        tf.pointer_button("left", "up", at=CENTER)
        tf.pointer_button("left", "down", at=FAR)
        wait_query(tf, f"{EXT}/report_count", 3, desc="the strayed press drained")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 1, "a strayed second press is a fresh single")
        tf.pointer_button("left", "up", at=FAR)

        # --- a real single click after all that still reads as 1 (recovery). A
        #     fresh MIDDLE button has no prior mark, so it is a clean single. ---
        clear(tf)
        tf.pointer_button("middle", "down", at=CENTER)
        wait_query(tf, f"{EXT}/report_count", 1, desc="a real edge recovers")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 1, "a fresh button press is a single")
        assert_eq(tf.query(f"{EXT}/last_button"), "middle", "the recovered button is identified")
        tf.pointer_button("middle", "up", at=CENTER)
        wait_query(tf, f"{EXT}/report_count", 2, desc="its release lifts the grab")
        assert_eq(tf.query(f"{EXT}/last_clicks"), 1, "the recovered release echoes 1")


if __name__ == "__main__":
    sys.exit(run_demo("r1422_double_click", body))
