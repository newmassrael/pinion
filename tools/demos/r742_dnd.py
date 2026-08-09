#!/usr/bin/env python3
"""R742 §5.51 — generic drag-and-drop substrate E2E.

Drives the new `hello-dnd` reorderable list via JSON-RPC. The list is the
first consumer of the R742 drag-and-drop substrate: the additive
`External::begin_drag` / `drag_to` / `drag_release` hooks plus the
`InputRouter` drag session that resolves the drop location under the
absolute cursor (the pointer-driven generalisation of the invoke-driven
dock `resolve_dock_drop`). Every row is both a drag source and a drop
target; a single `ReorderListExternal` coordinator owns the order, so the
source *is* the resolver.

The whole gesture is driven by the *existing* `scene/drag` RPC (press → N
interpolated cursor moves → release) — no new RPC method. The reorder is
observable as scene-as-data through `/external/order` + `/external/labels`.

Atomic verification scope (>=30 assertions):

  (A) boot — root `dnd` tag + four row tags `dnd#0..3` present; the order
      is identity `[0,1,2,3]`; labels are the declared order; no drag
      preview is in flight.
  (B) reorder by drag — dragging row 0 onto the lower half of row 2 moves
      it below (gap 3); dragging the last row onto the upper half of row 0
      moves it to the top (gap 0); labels track the order each time. The
      exact gap classification (top/bottom half) is unit-tested in the
      binding; here we assert the end-to-end order through `scene/drag`.
  (C) drop-on-self is a no-op — pressing and releasing a row over its own
      gap leaves the order unchanged (press-without-drag never reorders).
  (D) introspection — `/external/order` + `/external/labels` round-trip;
      `/external/preview` is null at rest; unknown path + intervene (the
      and (R1450) `order` takes a whole permutation back) are checked.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame shows four rows in identity order, each filled with its
    item's witness colour (red / green / blue / amber, top to bottom) and
    all four mutually distinct — the live-pixel proof that the visual
    order renders as authored (the reorder itself is proven by the RPC
    `/external/order` deltas above).
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    wait_until,
    sample_png_points,
)

EXAMPLE = "hello-dnd"
VIEWPORT = (300, 260)
TAG = "dnd"
ROWS = [f"dnd#{i}" for i in range(4)]
ITEM_LABELS = ["Alpha", "Bravo", "Charlie", "Delta"]
# Witness colours, matching `ITEMS` in the binding (item id -> RGB).
ITEM_RGB = [
    (0xE5, 0x39, 0x35),  # Alpha  red
    (0x43, 0xA0, 0x47),  # Bravo  green
    (0x1E, 0x88, 0xE5),  # Charlie blue
    (0xFB, 0xC0, 0x2D),  # Delta  amber
]


def order(tf) -> list:
    return tf.query("/external/order")


def labels(tf) -> list:
    return tf.query("/external/labels")


def row_rects(tf) -> dict:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return abs_rects_of(snap)


def labels_for(o: list) -> list:
    return [ITEM_LABELS[i] for i in o]


def drag_onto(tf, src: str, dst: str, frac_y: float) -> None:
    """Drag row `src` onto row `dst`, releasing at vertical fraction
    `frac_y` of `dst`'s rect (`<0.5` = upper half = insert above, `>0.5` =
    lower half = insert below)."""
    rects = row_rects(tf)
    sx, sy, sw, sh = rects[src]
    dx, dy, dw, dh = rects[dst]
    frm = (float(sx + sw // 2), float(sy + sh // 2))
    to = (float(dx + dw // 2), float(dy + int(dh * frac_y)))
    tf.drag(from_at=frm, to_at=to, steps=8)


def near_rgb(a: tuple, b: tuple, tol: int = 8) -> bool:
    return all(abs(int(a[i]) - int(b[i])) <= tol for i in range(3))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot shape ──────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, TAG) is not None, "list root present at boot"
        for r in ROWS:
            assert find_by_tag(snap, r) is not None, f"row {r} present at boot"
        assert_eq(order(tf), [0, 1, 2, 3], "boot order is identity")
        assert_eq(labels(tf), ITEM_LABELS, "boot labels in declaration order")
        assert tf.query("/external/preview") is None, "no drag preview at rest"

        # ── (B) reorder by drag ─────────────────────────────────────
        # Drag row 0 (Alpha) onto the LOWER half of row 2 → gap 3.
        drag_onto(tf, "dnd#0", "dnd#2", 0.75)
        wait_until(
            lambda: order(tf) == [1, 2, 0, 3],
            desc="Alpha dropped below row 2",
        )
        assert_eq(labels(tf), labels_for([1, 2, 0, 3]), "labels track reorder #1")
        assert tf.query("/external/preview") is None, "preview cleared after drop"

        # Now order is [1,2,0,3]. Drag the last row (visual 3 = Delta)
        # onto the UPPER half of row 0 → gap 0 (move to the very top).
        drag_onto(tf, "dnd#3", "dnd#0", 0.25)
        wait_until(
            lambda: order(tf) == [3, 1, 2, 0],
            desc="Delta dropped at the top",
        )
        assert_eq(labels(tf), labels_for([3, 1, 2, 0]), "labels track reorder #2")

        # ── (C) drop-on-self is a no-op ─────────────────────────────
        before = order(tf)
        drag_onto(tf, "dnd#1", "dnd#1", 0.25)  # press + release over self
        assert_eq(order(tf), before, "dropping a row on its own gap is a no-op")

        # ── (E) keyboard reorder (APG keyboard-drag, modifier-free) ──
        # R742.3 — clicking a composite row focuses the single-tab-stop
        # list (the click resolves to the sub-tag `dnd#0`, which falls back
        # to the focusable primary `dnd`); no explicit focus/set needed.
        tf.click(path="dnd#0")
        assert_eq(tf.request("focus/get").result.get("focused"), TAG, "click on a row focuses the list")
        base = order(tf)
        tf.key(path=TAG, name="ArrowDown")  # cursor → row 0
        wait_until(
            lambda: tf.query("/external/focused_index") == 0,
            desc="ArrowDown sets the cursor",
        )
        assert_eq(order(tf), base, "navigating the cursor does not reorder")
        # Space picks up the focused row; ArrowDown moves it; Space drops.
        tf.key(path=TAG, name=" ")
        wait_until(
            lambda: tf.query("/external/grabbed") is True,
            desc="Space grabs the focused row",
        )
        tf.key(path=TAG, name="ArrowDown")
        wait_until(
            lambda: order(tf) != base,
            desc="grabbed ArrowDown reorders",
        )
        moved = order(tf)
        assert_eq(tf.query("/external/focused_index"), 1, "cursor follows the grabbed row")
        tf.key(path=TAG, name=" ")
        wait_until(
            lambda: tf.query("/external/grabbed") is False,
            desc="Space drops the grab",
        )
        assert_eq(order(tf), moved, "dropped order is kept")
        # Escape cancels a grab, reverting to the pre-grab order.
        pre = order(tf)
        tf.key(path=TAG, name=" ")          # grab (cursor at 1)
        wait_until(
            lambda: tf.query("/external/grabbed") is True,
            desc="Space re-grabs for the Escape arc",
        )
        tf.key(path=TAG, name="End")        # move grabbed row to bottom
        wait_until(lambda: order(tf) != pre, desc="grabbed End moved the row")
        tf.key(path=TAG, name="Escape")
        wait_until(
            lambda: tf.query("/external/grabbed") is False,
            desc="Escape drops the grab",
        )
        assert_eq(order(tf), pre, "Escape reverts to the pre-grab order")

        # ── (D) introspection round-trip + rejections ───────────────
        assert_eq(
            labels(tf),
            [ITEM_LABELS[i] for i in order(tf)],
            "labels are exactly the item labels in visual order",
        )
        raised = False
        try:
            tf.query("/external/no_such_field")
        except RpcError:
            raised = True
        assert raised, "unknown introspect path must be rejected"
        # R1450 — `order` is a writable slot now (the column-header consumer needed
        # the toolkit's restoreState, and the model owns the permutation
        # check), so a real permutation lands and a non-permutation is refused.
        before = tf.query("/external/order")
        tf.intervene("/external/order", [3, 2, 1, 0])
        wait_until(lambda: tf.query("/external/order") == [3, 2, 1, 0],
                   desc="a saved order restores")
        raised = False
        try:
            tf.intervene("/external/order", [0, 0, 1, 2])
        except RpcError:
            raised = True
        assert raised, "a non-permutation is refused"
        assert_eq(tf.query("/external/order"), [3, 2, 1, 0], "and changes nothing")
        tf.intervene("/external/order", before)
        wait_until(lambda: tf.query("/external/order") == before,
                   desc="restore the pre-check order")

    # ── Phase 2 — live pixels (boot frame, identity order) ──────────
    rects = _boot_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    for r in ROWS:
        assert r in rects, f"row {r} has a paint rect for pixel sampling"
    # Sample each row left-of-centre (avoiding the centred label glyph) and
    # assert it matches that row's witness colour in identity order.
    samples = []
    for vis, r in enumerate(ROWS):
        rx, ry, rw, rh = rects[r]
        pt = (rx + max(6, rw // 8), ry + rh // 2)
        px = sample_png_points(img, [pt])[0]
        samples.append(px)
        assert near_rgb(px, ITEM_RGB[vis]), \
            f"row {vis} fill ~ {ITEM_RGB[vis]}, got {px[:3]}"
        assert px[3] == 255, f"row {vis} fill opaque, got alpha {px[3]}"
    # All four witness colours are mutually distinct (a reorder is visible).
    for i in range(len(samples)):
        for j in range(i + 1, len(samples)):
            assert samples[i][:3] != samples[j][:3], \
                f"rows {i} and {j} must differ ({samples[i][:3]} vs {samples[j][:3]})"


def _boot_rects() -> dict:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        return row_rects(tf)


def capture_screenshot() -> Path:
    """Render the boot frame to RGBA8 PNG via `PINION_SCREENSHOT`,
    bypassing winit (producer parity: the same `to_vello_cached`
    rasterizer the live window uses)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r742-")) / "dnd.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", EXAMPLE, "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R742 §5.51 — generic drag-and-drop substrate", body))
