#!/usr/bin/env python3
"""R1431 §5.35 §5.15 — the pointer DEVICE type on the raw surface.

This is the last member of Qt `QTabletEvent`: after pressure (R1423), tilt
(R1429), and twist / tangential / height (R1430), R1431 adds pointerType — the
device that produced the stream. `External::pointer_kind` forwards the W3C
`PointerEvent.pointerType` / Qt `QTabletEvent::pointerType()`: mouse / pen /
eraser / touch. The `eraser` variant is the stylus's eraser end (a Qt distinction
W3C folds into `pen`), so an eraser-aware surface flips to erase without a device
query.

winit does not classify the pointer device, so `scene/pointer_type` is the sole
driver (§2 #2): a headless AI client can present as a pen or eraser with no
tablet. hello-raw-pointer colours the pen-tip marker by device and badges the
readout; this demo drives the device over the wire and reads BOTH the
`pointer_type` introspect field AND the readout badge.

Run from the workspace root:
    cargo build -p hello-raw-pointer --release
    python3 tools/demos/r1431_pointer_type.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    RpcError,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_snap,
)

WIN = (720, 440)
PANE = "pane"
READOUT = "raw.readout"
EXT = "/external"

RECT_X, RECT_Y, RECT_W, RECT_H = 16, 48, WIN[0] - 32, WIN[1] - 104
CENTER = (RECT_X + 0.5 * RECT_W, RECT_Y + 0.5 * RECT_H)
RIGHT = (RECT_X + 0.8 * RECT_W, RECT_Y + 0.5 * RECT_H)


def readout(snap) -> str:
    node = find_by_tag(snap, READOUT)
    return (node or {}).get("content", "") if node else ""


def body() -> None:
    with RpcSubprocess("hello-raw-pointer") as tf:
        wait_snap(
            tf,
            lambda s: find_by_tag(s, PANE) is not None,
            source="paint",
            viewport=WIN,
            desc="pane surface resolved",
        )

        # --- boot: the default device is a mouse ---
        assert_eq(tf.query(f"{EXT}/pointer_type"), "mouse", "boot: device is mouse")

        # --- position + one raw button edge so the readout shows a live report
        #     to badge (a single edge = one report = "#1"). ---
        tf.hover(at=CENTER)
        tf.pointer_button("left", "down", at=CENTER)
        wait_snap(
            tf,
            lambda s: "#1" in readout(s),
            source="paint",
            viewport=WIN,
            desc="a report shows in the readout",
        )

        # --- drive each device kind; the introspect field + readout track it. ---
        for kind in ["pen", "eraser", "touch"]:
            tf.pointer_type(kind)
            wait_query(tf, f"{EXT}/pointer_type", kind, desc=f"device -> {kind}")
            snap = wait_snap(
                tf,
                lambda s, k=kind: f"· {k}" in readout(s),
                source="paint",
                viewport=WIN,
                desc=f"the readout badges {kind}",
            )
            assert f"· {kind}" in readout(snap), f"readout names {kind}"

        # --- back to mouse: the readout drops the device badge. ---
        tf.pointer_type("mouse")
        wait_query(tf, f"{EXT}/pointer_type", "mouse", desc="device -> mouse")
        snap = wait_snap(
            tf,
            lambda s: "· pen" not in readout(s) and "· eraser" not in readout(s),
            source="paint",
            viewport=WIN,
            desc="a mouse drops the device badge",
        )
        assert "eraser" not in readout(snap), "no device badge for a plain mouse"

        # --- the device RIDES a move (it is per-pointer state, not per-event). ---
        tf.pointer_type("eraser")
        wait_query(tf, f"{EXT}/pointer_type", "eraser", desc="eraser set for the move check")
        tf.hover(at=RIGHT)
        wait_snap(
            tf,
            lambda s: abs(float(tf.query(f"{EXT}/x_frac") or -1) - 0.8) < 0.03,
            source="paint",
            viewport=WIN,
            desc="the pointer moves as an eraser",
        )
        assert_eq(tf.query(f"{EXT}/pointer_type"), "eraser", "device persists across a move")

        # --- wire contract: an unknown / missing / non-string device rejects. ---
        try:
            tf.request("scene/pointer_type", {"type": "stylus"})
            raise AssertionError("an unknown device must be rejected")
        except RpcError as exc:
            print(f"  ok: unknown device rejected ({exc.message!r})")
        try:
            tf.request("scene/pointer_type", {})
            raise AssertionError("a missing type must be rejected")
        except RpcError as exc:
            print(f"  ok: missing type rejected ({exc.message!r})")
        try:
            tf.request("scene/pointer_type", {"type": 3})
            raise AssertionError("a non-string type must be rejected")
        except RpcError as exc:
            print(f"  ok: non-string type rejected ({exc.message!r})")

        # --- recovery: a valid device after the errors still reads. ---
        tf.pointer_type("pen")
        wait_query(tf, f"{EXT}/pointer_type", "pen", desc="a real device recovers after errors")


if __name__ == "__main__":
    sys.exit(run_demo("r1431_pointer_type", body))
