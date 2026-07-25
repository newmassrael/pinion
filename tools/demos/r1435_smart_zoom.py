#!/usr/bin/env python3
"""R1435 §5.35 §5.15 — a native SMART-ZOOM gesture fits the block under the finger.

`External::smart_zoom_gesture` forwards the Qt `QNativeGestureEvent`
`SmartZoomNativeGesture` / macOS `smartMagnifyWithEvent:` / winit
`WindowEvent::DoubleTapGesture` peer — the family's PHASE-LESS member. Where
pinch / rotation / pan each accumulate a delta across a begin..end arc and
discard it on cancel, this one carries no payload and no phase at all: the
platform reports ONE completed toggle. What it does carry is the anchor, and the
anchor is the point — it selects WHICH object gets fitted, which is the whole
meaning of "smart" zoom.

This demo drives a three-block document page: a gesture zooms the block under
the cursor to fill the page, the same block again restores fit-to-page, and a
DIFFERENT block re-targets the zoom instead of zooming out. Both the
`focused_block` introspect field (§2 #7) and the blocks' paint rects are read,
so the data and the picture are checked against each other.

winit surfaces `DoubleTapGesture` only on macOS / iOS, so
`scene/smart_zoom_gesture` is the sole driver headless (§2 #2).

Run from the workspace root:
    cargo build -p hello-smart-zoom --release
    python3 tools/demos/r1435_smart_zoom.py
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
    wait_snap,
)

WIN = (640, 460)
PAGE = "page"
EXT = "/external"

# The page geometry mirrors the Rust constants.
PAGE_X, PAGE_Y, PAGE_W, PAGE_H = 20, 56, WIN[0] - 40, WIN[1] - 130
BLOCK_COUNT = 3

# A point inside block i: the vertical centre of its third of the page. The
# gesture's y_rel fraction is what selects the block, so these coordinates are
# the demo's real subject.
def point_in_block(i: int) -> tuple[float, float]:
    return (PAGE_X + 0.5 * PAGE_W, PAGE_Y + PAGE_H * (i + 0.5) / BLOCK_COUNT)


def block_rect(snap, i: int):
    node = find_by_tag(snap, f"zoom.block.{i}")
    return node["rect"] if node else None


def fits_page(rect) -> bool:
    return (
        rect is not None
        and abs(rect["y"] - PAGE_Y) <= 1
        and abs(rect["h"] - PAGE_H) <= 1
        and abs(rect["w"] - PAGE_W) <= 1
    )


def body() -> None:
    with RpcSubprocess("hello-smart-zoom") as tf:
        base = wait_snap(
            tf,
            lambda s: find_by_tag(s, PAGE) is not None and block_rect(s, 2) is not None,
            source="paint",
            viewport=WIN,
            desc="page + all three blocks resolved",
        )

        # --- boot: fit-to-page, every block present and none filling it. ---
        assert_eq(tf.query(f"{EXT}/zoomed"), False, "boot: not zoomed")
        assert_eq(tf.query(f"{EXT}/focused_block"), None, "boot: no focused block")
        assert_eq(tf.query(f"{EXT}/events"), 0, "boot: zero gestures")
        assert_eq(tf.query(f"{EXT}/anchor_x"), None, "boot: no anchor yet")
        for i in range(BLOCK_COUNT):
            r = block_rect(base, i)
            assert r is not None, f"block {i} painted at boot"
            assert not fits_page(r), f"block {i} does not fill the page at boot"
        tops = [block_rect(base, i)["y"] for i in range(BLOCK_COUNT)]
        assert tops == sorted(tops), f"blocks stack downward: {tops}"
        print(f"  ok: boot fit-to-page, block tops {tops}")

        # --- one gesture over block 1 zooms THAT block to fill the page. ---
        tf.smart_zoom_gesture(at=point_in_block(1))
        assert_eq(tf.query(f"{EXT}/focused_block"), 1, "zoomed to the block under the cursor")
        assert_eq(tf.query(f"{EXT}/zoomed"), True, "zoomed flag set")
        assert_eq(tf.query(f"{EXT}/events"), 1, "one gesture")
        zoomed = wait_snap(
            tf,
            lambda s: fits_page(block_rect(s, 1)),
            source="paint",
            viewport=WIN,
            desc="block 1 fills the page",
        )
        assert block_rect(zoomed, 0) is None, "block 0 is out of the scene while zoomed"
        assert block_rect(zoomed, 2) is None, "block 2 is out of the scene while zoomed"
        print(f"  ok: block 1 fills the page {block_rect(zoomed, 1)}")

        # --- the SAME block again restores fit-to-page (the tap-again rule). ---
        tf.smart_zoom_gesture(at=point_in_block(1))
        assert_eq(tf.query(f"{EXT}/focused_block"), None, "same block again -> fit-to-page")
        assert_eq(tf.query(f"{EXT}/zoomed"), False, "no longer zoomed")
        assert_eq(tf.query(f"{EXT}/events"), 2, "two gestures")
        restored = wait_snap(
            tf,
            lambda s: all(block_rect(s, i) is not None for i in range(BLOCK_COUNT))
            and not fits_page(block_rect(s, 1)),
            source="paint",
            viewport=WIN,
            desc="all three blocks are back at fit-to-page size",
        )
        print(f"  ok: restored, block 1 back to {block_rect(restored, 1)}")

        # --- a DIFFERENT block re-targets instead of zooming out. This is what
        # separates a smart zoom from a binary in/out toggle. ---
        tf.smart_zoom_gesture(at=point_in_block(0))
        assert_eq(tf.query(f"{EXT}/focused_block"), 0, "zoomed to block 0")
        tf.smart_zoom_gesture(at=point_in_block(2))
        assert_eq(tf.query(f"{EXT}/focused_block"), 2, "a different block RE-TARGETS")
        assert_eq(tf.query(f"{EXT}/zoomed"), True, "still zoomed, not restored")
        retargeted = wait_snap(
            tf,
            lambda s: fits_page(block_rect(s, 2)) and block_rect(s, 0) is None,
            source="paint",
            viewport=WIN,
            desc="block 2 now fills the page",
        )
        print(f"  ok: re-targeted to block 2 {block_rect(retargeted, 2)}")
        tf.smart_zoom_gesture(at=point_in_block(2))
        assert_eq(tf.query(f"{EXT}/focused_block"), None, "and that block again restores")

        # --- the anchor is recorded, and it is what selected the block. ---
        tf.invoke(f"{EXT}/reset", None)
        top_left = (PAGE_X + 0.2 * PAGE_W, PAGE_Y + 0.1 * PAGE_H)
        tf.smart_zoom_gesture(at=top_left)
        ax = float(tf.query(f"{EXT}/anchor_x"))
        ay = float(tf.query(f"{EXT}/anchor_y"))
        assert abs(ax - 0.2) < 1e-6, f"anchor x fraction 0.2, got {ax}"
        assert abs(ay - 0.1) < 1e-6, f"anchor y fraction 0.1, got {ay}"
        assert_eq(tf.query(f"{EXT}/focused_block"), 0, "y_rel 0.1 selects the first block")
        print(f"  ok: anchor ({ax}, {ay}) selected block 0")
        # The bottom edge of the page selects the LAST block, not a fourth one.
        tf.invoke(f"{EXT}/reset", None)
        tf.smart_zoom_gesture(at=(PAGE_X + 0.5 * PAGE_W, PAGE_Y + PAGE_H - 1))
        assert_eq(tf.query(f"{EXT}/focused_block"), 2, "the bottom edge clamps to the last block")

        # --- path targeting resolves the page tag, same as `at`. ---
        tf.invoke(f"{EXT}/reset", None)
        tf.smart_zoom_gesture(path=PAGE)
        assert_eq(tf.query(f"{EXT}/events"), 1, "a path-addressed gesture landed")
        assert_eq(tf.query(f"{EXT}/zoomed"), True, "path targeting zooms the page centre")
        assert_eq(tf.query(f"{EXT}/focused_block"), 1, "the tag's centre is block 1")

        # --- modifiers ride the out-of-band cache. ---
        tf.invoke(f"{EXT}/reset", None)
        tf.modifiers(shift=True)
        tf.smart_zoom_gesture(at=point_in_block(0))
        assert "s" in str(tf.query(f"{EXT}/last_mods")), "shift reached the smart-zoom hook"
        tf.modifiers()  # release
        tf.smart_zoom_gesture(at=point_in_block(0))
        assert_eq(tf.query(f"{EXT}/last_mods"), "", "modifiers cleared on release")

        # --- reset: back to fit-to-page, history cleared. ---
        tf.invoke(f"{EXT}/reset", None)
        assert_eq(tf.query(f"{EXT}/zoomed"), False, "reset restores fit-to-page")
        assert_eq(tf.query(f"{EXT}/events"), 0, "reset clears the gesture count")
        assert_eq(tf.query(f"{EXT}/anchor_x"), None, "reset clears the anchor")

        # --- wire contract: the target is the only thing that can be wrong. ---
        for params, why in [
            ({}, "no target at all"),
            ({"at": {"x": 100.0}}, "at missing y"),
            ({"path": 42}, "non-string path"),
            ({"path": "no.such.tag"}, "unknown tag"),
        ]:
            try:
                tf.request("scene/smart_zoom_gesture", params)
                raise AssertionError(f"{why} must be rejected")
            except RpcError as exc:
                print(f"  ok: {why} rejected ({exc.message!r})")
        assert_eq(tf.query(f"{EXT}/events"), 0, "no rejected call reached the widget")

        # --- a stray `phase` key is IGNORED, not rejected: this gesture has no
        # lifecycle, and pretending to validate one would advertise an arc that
        # does not exist. ---
        tf.request(
            "scene/smart_zoom_gesture",
            {"at": {"x": point_in_block(1)[0], "y": point_in_block(1)[1]}, "phase": "begin"},
        )
        assert_eq(tf.query(f"{EXT}/events"), 1, "a stray phase key is ignored, the gesture lands")
        assert_eq(tf.query(f"{EXT}/focused_block"), 1, "and it zoomed the anchored block")

        # --- recovery: a valid gesture after the rejects still toggles. ---
        tf.smart_zoom_gesture(at=point_in_block(1))
        assert_eq(tf.query(f"{EXT}/zoomed"), False, "a real gesture recovers after the rejects")


if __name__ == "__main__":
    sys.exit(run_demo("r1435_smart_zoom", body))
