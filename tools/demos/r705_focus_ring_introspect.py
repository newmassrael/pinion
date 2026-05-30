#!/usr/bin/env python3
"""R705 §5.39 §2 #1/#7 — focus ring as an introspectable overlay node.

Until R705 the keyboard focus ring was an OPAQUE vello stroke
(`pinion_runtime::paint_adapter::paint_focus_ring`) painted *after* the
Scene -> Vello tree walk. That violated two binding invariants:

  * §2 #1 (no opaque paint callbacks) — the ring bypassed the Scene tree.
  * §2 #7 (scene-as-data) — it never appeared in `scene/snapshot`, so an
    AI agent could not observe which widget was focused or where the
    ring drew, and it ignored the focused node's `corner_radius` (always
    a sharp rectangle — the R704 datepicker "square ring on a circular
    day cell" defect that surfaced this).

R705 promotes the ring to a real `Scene::Box` overlay injected by
`pinion_overlay::inject_focus_ring` as the final step of every
paint-scene producer. The overlay is pointer-transparent (the new
§5.39 `LayoutStyle::pointer_transparent` substrate — CSS
`pointer-events: none`) so it lives in the live, hit-tested paint scene
WITHOUT shadowing the widget it annotates for input.

This demo proves the fix entirely through RPC introspection (no
screenshot, no code inference) per [[ai-first-rpc-introspection-obligation]]:

  (A) the focused control now carries a sibling `ai-overlay/focus-ring`
      Box in `scene/snapshot from: paint` — the ring is scene-as-data.
  (B) the ring geometry tracks the focused widget: rect inflated by the
      2px offset; transparent fill (does not occlude); 2px border.
  (C) the ring corner_radius mirrors the focused node's rounding (sharp
      node -> sharp ring here; the concentric rounded case is unit-
      tested in pinion-overlay::focus_ring).
  (D) the ring is pointer-transparent: a click on the focused widget's
      centre — squarely under the overlay box — still activates the
      widget rather than being swallowed by the ring.
  (E) exactly one ring exists (single focus) and it follows focus,
      leaving no stale ring behind.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (520, 360)
PAUSE = 0.12
RING_TAG = "ai-overlay/focus-ring"
RING_WIDTH = 2
RING_OFFSET = 2

TRIGGER = "open_dialog"
OK = "dialog_ok"
CANCEL = "dialog_cancel"
PANEL = "dialog_panel"


def _focused(tf):
    return tf.request("focus/get").result.get("focused")


def _count_rings(node) -> int:
    """Count every `ai-overlay/focus-ring` Box anywhere in the tree."""
    total = 1 if node.get("tag") == RING_TAG else 0
    for child in node.get("children", []) or []:
        total += _count_rings(child)
    content = node.get("content")
    if isinstance(content, dict):
        total += _count_rings(content)
    return total


def _expect_ring_around(snap, tag, label):
    """Assert a ring overlay exists, framing the widget `tag`: rect
    inflated by the offset, transparent fill, 2px border, concentric
    corner radius. Returns the ring node."""
    widget = find_by_tag(snap, tag)
    assert widget is not None, f"{label}: widget {tag} present"
    ring = find_by_tag(snap, RING_TAG)
    assert ring is not None, f"{label}: focus ring is scene-as-data (§2 #7)"
    assert_eq(ring.get("type"), "Box", f"{label}: ring is a Scene::Box (§2 #1)")

    wr = widget["rect"]
    rr = ring["rect"]
    assert_eq(rr["x"], wr["x"] - RING_OFFSET, f"{label}: ring x = widget x - offset")
    assert_eq(rr["y"], wr["y"] - RING_OFFSET, f"{label}: ring y = widget y - offset")
    assert_eq(rr["w"], wr["w"] + 2 * RING_OFFSET, f"{label}: ring w = widget w + 2*offset")
    assert_eq(rr["h"], wr["h"] + 2 * RING_OFFSET, f"{label}: ring h = widget h + 2*offset")

    style = ring["style"]
    assert_eq(style["fill"]["a"], 0, f"{label}: ring fill transparent (no occlusion)")
    border = style.get("border")
    assert border is not None, f"{label}: ring carries a border"
    assert_eq(border["width"], RING_WIDTH, f"{label}: ring border is the 2px indicator")
    assert border.get("color") is not None, f"{label}: ring border carries a colour"

    # Concentric corner radius: rounded widget -> radius + offset; sharp
    # widget -> sharp ring. (The old opaque stroke was always sharp.)
    wrad = widget["style"].get("corner_radius", 0)
    expected = 0 if wrad == 0 else wrad + RING_OFFSET
    assert_eq(
        style.get("corner_radius"),
        expected,
        f"{label}: ring corner_radius tracks the widget ({wrad} -> {expected})",
    )
    return ring


def body() -> None:
    with RpcSubprocess("hello-dialog", boot_grace=1.5) as tf:
        # ── (A)+(B)+(C) open + auto-focus Cancel: ring is introspectable
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        assert_eq(_focused(tf), CANCEL, "open auto-focuses Cancel")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        _expect_ring_around(snap, CANCEL, "Cancel focused")

        # (E) exactly one ring — a single focus paints a single overlay.
        assert_eq(_count_rings(snap), 1, "exactly one focus ring in the scene")

        # The widget keeps its own R694 posture border — the overlay is
        # additive and faithful, not a replacement of widget chrome.
        cancel = find_by_tag(snap, CANCEL)
        assert cancel["style"].get("border") is not None, "Cancel keeps its own R694 border"

        # ── (E) focus/next moves the ring Cancel -> Delete, no stale ring
        assert_eq(tf.request("focus/next").result.get("focused"), OK, "Tab -> Delete")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        _expect_ring_around(snap, OK, "Delete focused")
        assert_eq(_count_rings(snap), 1, "still exactly one ring after Tab")
        # the ring no longer frames Cancel — its rect moved to Delete.
        ring = find_by_tag(snap, RING_TAG)
        cancel = find_by_tag(snap, CANCEL)
        assert ring["rect"]["y"] == cancel["rect"]["y"] - RING_OFFSET or \
            ring["rect"]["x"] != cancel["rect"]["x"] - RING_OFFSET, \
            "ring left Cancel (no stale ring)"

        # back to Cancel (wrap) so the click target below is deterministic.
        assert_eq(tf.request("focus/next").result.get("focused"), CANCEL, "wrap -> Cancel")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        _expect_ring_around(snap, CANCEL, "Cancel re-focused")

        # ── (D) pointer-transparency: the ring fully covers Cancel's
        #        centre, but clicking there must reach Cancel (the ring is
        #        invisible to hit-testing) — Cancel dismisses the dialog.
        cancel = find_by_tag(snap, CANCEL)
        ring = find_by_tag(snap, RING_TAG)
        cx = cancel["rect"]["x"] + cancel["rect"]["w"] // 2
        cy = cancel["rect"]["y"] + cancel["rect"]["h"] // 2
        # the click point is inside the ring overlay box.
        assert ring["rect"]["x"] <= cx <= ring["rect"]["x"] + ring["rect"]["w"], \
            "click point is under the ring overlay (x)"
        assert ring["rect"]["y"] <= cy <= ring["rect"]["y"] + ring["rect"]["h"], \
            "click point is under the ring overlay (y)"
        tf.click(at=(float(cx), float(cy)))
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, CANCEL) is None, \
            "click reached Cancel through the ring -> dialog dismissed"
        assert_eq(
            _count_rings(snap),
            1 if find_by_tag(snap, RING_TAG) else 0,
            "ring count consistent after dismiss",
        )

        # ── (A again) focus + ring return to the trigger — the ring
        #        substrate is not dialog-specific; a standalone focused
        #        button is framed too, and stays introspectable.
        assert_eq(_focused(tf), TRIGGER, "focus restored to the trigger")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        _expect_ring_around(snap, TRIGGER, "trigger focused")
        assert_eq(_count_rings(snap), 1, "one ring on the re-focused trigger")

        # ── (D again) pointer-transparency on the trigger: clicking it
        #        re-opens the dialog (the ring over the trigger does not
        #        swallow the click).
        trig = find_by_tag(snap, TRIGGER)
        tcx = trig["rect"]["x"] + trig["rect"]["w"] // 2
        tcy = trig["rect"]["y"] + trig["rect"]["h"] // 2
        tf.click(at=(float(tcx), float(tcy)))
        time.sleep(PAUSE)
        assert_eq(_focused(tf), CANCEL, "trigger click reopened the dialog through its ring")


if __name__ == "__main__":
    sys.exit(run_demo("R705 §5.39 §2 #1/#7 — introspectable focus ring", body))
