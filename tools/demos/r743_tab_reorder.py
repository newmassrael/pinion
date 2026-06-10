#!/usr/bin/env python3
"""R743 §5.51 — drag-and-drop substrate, second consumer (reorderable tabs).

Drives the new `hello-tab-reorder` Material 3 tab strip via JSON-RPC. It is
the *second* consumer of the R742 drag-and-drop substrate (the additive
`External::begin_drag` / `drag_to` / `drag_release` hooks + the
`InputRouter` drag session), orthogonal to `hello-dnd`: a **horizontal**
strip whose items also carry a **selection**. The shared reorder mechanics
live in the lifted `pinion_core::widgets::reorder::ReorderModel`; this
binding embeds one (horizontal axis) and layers selection on top.

Selection is stored by **stable tab id**, so a reorder is a pure
permutation that never desyncs the selection — the active tab visually
follows when it moves, and dragging a tab activates it (VSCode/Chrome
behaviour). A click is a zero-distance drag, so click-to-select runs the
same `drag_release` path.

The whole gesture rides the *existing* `scene/drag` RPC (press → N
interpolated moves → release) — no new RPC method. The reorder is
observable as scene-as-data through `/external/order` + `/external/labels`
+ `/external/selected_id`.

Atomic verification scope (>=30 assertions):

  (A) boot — root `tabreorder` tag + four tab tags `tabreorder#0..3` +
      the `tabreorder_panel`; identity order; labels in declaration order;
      tab 0 selected (id + visual); no drag preview.
  (B) reorder by horizontal drag — dragging tab 0 onto the right half of
      tab 2 moves it after (order [1,2,0,3]); the *dragged* tab stays
      selected and now sits at its new visual position; labels track.
  (C) drop-on-self — pressing + releasing a tab over its own gap leaves
      the order unchanged but selects that tab (click == zero-distance
      drag).
  (D) selection is a stable id under permutation — clicking selects by id;
      dragging a different tab re-activates the dragged one.
  (E) keyboard — click focuses the strip; Arrow activates the adjacent
      tab; Space grabs, Arrow reorders (cursor + selection follow), Space
      drops; Escape reverts a grab.
  (F) introspection — order / labels / selected_id / selected_visual
      round-trip; unknown path + read-only `order` intervene are rejected;
      `selected_id` is a writable form-default slot.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame shows four tabs, each carrying its id's witness swatch
    (green / blue / amber / red, left to right) — all mutually distinct,
    the live-pixel proof the visual order renders as authored (the reorder
    itself is proven by the `/external/order` deltas above).
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
    sample_png_points,
    wait_until,
)

EXAMPLE = "hello-tab-reorder"
VIEWPORT = (520, 300)
TAG = "tabreorder"
PANEL = "tabreorder_panel"
N = 4
TABS = [f"tabreorder#{i}" for i in range(N)]
TAB_LABELS = ["Scene", "Script", "Shader", "Assets"]
# Per-id witness swatch colours, matching `TABS` in the binding.
ITEM_RGB = [
    (0x43, 0xA0, 0x47),  # Scene  green
    (0x1E, 0x88, 0xE5),  # Script blue
    (0xFB, 0xC0, 0x2D),  # Shader amber
    (0xE5, 0x39, 0x35),  # Assets red
]
# Swatch geometry inside each tab (binding `tab_padding` + SWATCH/2).
SWATCH_DX = 16 + 6


def order(tf) -> list:
    return tf.query("/external/order")


def labels(tf) -> list:
    return tf.query("/external/labels")


def selected_id(tf) -> int:
    return tf.query("/external/selected_id")


def selected_visual(tf) -> int:
    return tf.query("/external/selected_visual")


def tab_rects(tf) -> dict:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return abs_rects_of(snap)


def labels_for(o: list) -> list:
    return [TAB_LABELS[i] for i in o]


def drag_onto(tf, src: str, dst: str, frac_x: float) -> None:
    """Drag tab `src` onto tab `dst`, releasing at horizontal fraction
    `frac_x` of `dst`'s rect (`<0.5` = left half = insert before, `>0.5` =
    right half = insert after)."""
    rects = tab_rects(tf)
    sx, sy, sw, sh = rects[src]
    dx, dy, dw, dh = rects[dst]
    frm = (float(sx + sw // 2), float(sy + sh // 2))
    to = (float(dx + int(dw * frac_x)), float(dy + dh // 2))
    tf.drag(from_at=frm, to_at=to, steps=8)


def near_rgb(a: tuple, b: tuple, tol: int = 8) -> bool:
    return all(abs(int(a[i]) - int(b[i])) <= tol for i in range(3))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot shape ──────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, TAG) is not None, "strip root present at boot"
        assert find_by_tag(snap, PANEL) is not None, "panel present at boot"
        for t in TABS:
            assert find_by_tag(snap, t) is not None, f"tab {t} present at boot"
        assert_eq(order(tf), [0, 1, 2, 3], "boot order is identity")
        assert_eq(labels(tf), TAB_LABELS, "boot labels in declaration order")
        assert_eq(selected_id(tf), 0, "tab 0 selected at boot")
        assert_eq(selected_visual(tf), 0, "selected tab sits at visual 0")
        assert tf.query("/external/preview") is None, "no drag preview at rest"

        # ── (B) reorder by horizontal drag ──────────────────────────
        # Drag tab 0 (Scene, id 0) onto the RIGHT half of tab 2 → gap 3.
        drag_onto(tf, "tabreorder#0", "tabreorder#2", 0.75)
        wait_until(
            lambda: order(tf) == [1, 2, 0, 3],
            desc="Scene dropped after tab 2",
        )
        assert_eq(labels(tf), labels_for([1, 2, 0, 3]), "labels track reorder #1")
        assert_eq(selected_id(tf), 0, "dragged tab (Scene) stays selected by id")
        assert_eq(selected_visual(tf), 2, "selected tab now sits at visual 2")
        assert tf.query("/external/preview") is None, "preview cleared after drop"

        # Order is [1,2,0,3]. Drag the last tab (visual 3 = Assets, id 3)
        # onto the LEFT half of tab 0 → gap 0 (move to the front).
        drag_onto(tf, "tabreorder#3", "tabreorder#0", 0.25)
        wait_until(
            lambda: order(tf) == [3, 1, 2, 0],
            desc="Assets dropped at the front",
        )
        assert_eq(selected_id(tf), 3, "dragging Assets activated it")
        assert_eq(selected_visual(tf), 0, "Assets now at visual 0")

        # ── (C) drop-on-self selects, does not reorder ──────────────
        before = order(tf)
        drag_onto(tf, "tabreorder#1", "tabreorder#1", 0.25)
        assert_eq(order(tf), before, "dropping a tab on its own gap is a no-op")
        # Visual 1 in [3,1,2,0] is id 1 (Script) — the click selected it.
        assert_eq(selected_id(tf), before[1], "click selected the clicked tab")

        # ── (D) selection is a stable id under permutation ──────────
        # Select Shader (id 2) by clicking its current visual position.
        vis_of_2 = order(tf).index(2)
        tf.click(path=f"tabreorder#{vis_of_2}")
        wait_until(lambda: selected_id(tf) == 2, desc="clicked Shader is selected")

        # ── (E) keyboard (Arrow activates; Space-grab + Arrow reorders) ─
        assert_eq(tf.request("focus/get").result.get("focused"), TAG,
                  "click on a tab focuses the strip")
        # Home anchors the cursor at visual 0 deterministically (a prior
        # drag left it at an unknown position) and activates that tab.
        tf.key(path=TAG, name="Home")
        wait_until(
            lambda: selected_id(tf) == order(tf)[0],
            desc="Home activates the first tab",
        )
        o0 = order(tf)
        tf.key(path=TAG, name="ArrowRight")
        wait_until(
            lambda: selected_id(tf) == order(tf)[1],
            desc="ArrowRight activates the next tab",
        )
        assert_eq(order(tf), o0, "navigation does not reorder")
        # Back to visual 0, grab, move right — a move from 0 always reorders.
        tf.key(path=TAG, name="Home")
        wait_until(
            lambda: selected_id(tf) == order(tf)[0],
            desc="Home re-anchors the cursor at visual 0",
        )
        grab_sel = selected_id(tf)
        tf.key(path=TAG, name=" ")
        wait_until(
            lambda: tf.query("/external/grabbed") is True,
            desc="Space grabs the focused tab",
        )
        ord_grab = order(tf)
        tf.key(path=TAG, name="ArrowRight")
        wait_until(
            lambda: order(tf) != ord_grab,
            desc="grabbed ArrowRight reorders the tab",
        )
        assert_eq(selected_id(tf), grab_sel, "the grabbed (moved) tab stays selected")
        tf.key(path=TAG, name=" ")
        wait_until(
            lambda: tf.query("/external/grabbed") is False,
            desc="Space drops the grab",
        )
        # Escape reverts a grab (cursor is now at visual 1 after the move).
        pre = order(tf)
        tf.key(path=TAG, name=" ")
        wait_until(
            lambda: tf.query("/external/grabbed") is True,
            desc="Space re-grabs for the Escape arc",
        )
        tf.key(path=TAG, name="End")
        wait_until(lambda: order(tf) != pre, desc="grabbed End moved the tab")
        tf.key(path=TAG, name="Escape")
        wait_until(
            lambda: tf.query("/external/grabbed") is False,
            desc="Escape drops the grab",
        )
        assert_eq(order(tf), pre, "Escape reverts to the pre-grab order")

        # ── (F) introspection round-trip + rejections ───────────────
        assert_eq(labels(tf), [TAB_LABELS[i] for i in order(tf)],
                  "labels are the tab labels in visual order")
        # selected_id is a writable form-default slot.
        tf.intervene("/external/selected_id", 1)
        assert_eq(selected_id(tf), 1, "selected_id is writable (form default / restore)")
        raised = False
        try:
            tf.query("/external/no_such_field")
        except RpcError:
            raised = True
        assert raised, "unknown introspect path must be rejected"
        raised = False
        try:
            tf.intervene("/external/order", [0, 1, 2, 3])
        except RpcError:
            raised = True
        assert raised, "order has no writable slot (drag-only) — intervene rejected"

    # ── Phase 2 — live pixels (boot frame, identity order swatches) ──
    rects = _boot_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    for t in TABS:
        assert t in rects, f"tab {t} has a paint rect for pixel sampling"
    samples = []
    for vis, t in enumerate(TABS):
        tx, ty, _tw, th = rects[t]
        pt = (tx + SWATCH_DX, ty + th // 2)
        px = sample_png_points(img, [pt])[0]
        samples.append(px)
        assert near_rgb(px, ITEM_RGB[vis]), \
            f"tab {vis} swatch ~ {ITEM_RGB[vis]}, got {px[:3]}"
    # All four witness swatches are mutually distinct (a reorder is visible).
    for i in range(len(samples)):
        for j in range(i + 1, len(samples)):
            assert samples[i][:3] != samples[j][:3], \
                f"tabs {i} and {j} must differ ({samples[i][:3]} vs {samples[j][:3]})"


def _boot_rects() -> dict:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        return tab_rects(tf)


def capture_screenshot() -> Path:
    """Render the boot frame to RGBA8 PNG via `PINION_SCREENSHOT`,
    bypassing winit (producer parity: the same `to_vello_cached`
    rasterizer the live window uses)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r743-")) / "tabs.png"
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
    sys.exit(run_demo("R743 §5.51 — reorderable tabs (DnD substrate 2nd consumer)", body))
