#!/usr/bin/env python3
"""R705.1 §5.39 §5.45 §2 #7 — focus-ring placement, grounded across all examples.

Why this demo exists
====================

R705 made `scene/snapshot from: paint` read the STORED paint scene (the
displayed frame) instead of re-rendering at query time, so introspection
equals the screen by construction. With that in place, a latent bug in
the focus-ring overlay became both visible AND verifiable: a focused
widget inside a `Scene::Scroll` (a listbox row, a tree row, a control
below the fold) carries a *scroll-local* post-layout rect, but the ring
is a top-level overlay painted in *window-absolute* space. The pre-R705.1
overlay placed the ring at the raw scroll-local rect, so it drew tens of
pixels off the widget on screen.

The earlier per-demo ring assertions could not catch this: they compared
the ring rect against the focused widget rect, but BOTH were read from
the same snapshot in the same (scroll-local) coordinate space, so the
relationship `ring == widget +/- offset` held tautologically regardless of
where either actually painted on screen.

This demo grounds the check. For every focusable tab stop in every GUI
example, it:

  1. focuses the tab stop,
  2. reads `scene/snapshot from: paint` (= the displayed frame, R705),
  3. independently recomputes the WINDOW-ABSOLUTE rect of every tagged
     node by accumulating each enclosing `Scroll`'s
     `(viewport - offset)` translation (`abs_rects_of`), and
  4. asserts the ring overlay is the saturating-inflate of exactly one
     node's window-absolute rect — i.e. it concentrically frames a node
     that truly paints at that position (`assert_focus_ring_concentric`).

A ring drawn at a scroll-local rect frames no real absolute node and
fails. A scrolled-listbox sub-case exercises the non-zero-offset path
explicitly. The fix consolidates the scroll-offset translation into
`pinion_core::scene::Scene::rect_for_tag_absolute`, used by both the RPC
click/drag resolver and the focus-ring overlay, so where input lands and
where the ring paints can never drift.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    assert_focus_ring_concentric,
    run_demo,
)

# Every GUI example that seeds a `FocusManager` tab order. TUI siblings
# are excluded (no winit paint scene over RPC). Multi-window / dock
# examples are excluded — their focus model spans windows and is covered
# by their own demos.
GUI_EXAMPLES = [
    "hello-button",
    "figma-button-m3",
    "hello-checkbox",
    "hello-toggle",
    "hello-slider",
    "hello-slider-vertical",
    "hello-radio",
    "hello-radio-group",
    "hello-listbox",
    "hello-listbox-multi",
    "hello-textfield",
    "hello-tabs",
    "hello-menu",
    "hello-toolbar",
    "hello-dialog",
    "hello-drawer",
    "hello-tooltip",
    "hello-disclosure",
    "hello-accordion",
    "hello-accordion-single",
    "hello-tree-view",
    "hello-datepicker",
    "settings-panel",
    "todomvc",
]


def _sweep_example(example: str) -> tuple[int, int]:
    """Focus every tab stop; assert each drawn ring is concentric.

    Returns `(rings_checked, tab_stops)`.
    """
    rings = 0
    with RpcSubprocess(example, boot_grace=1.0) as app:
        order = app.request("focus/get").result.get("tab_order") or []
        for tag in order:
            try:
                app.request("focus/set", {"tag": tag})
            except RpcError:
                # A tag that is not focusable in the current scope (a
                # modal trap member while the modal is closed, etc.) —
                # skip; other tab stops still exercise the path.
                continue
            time.sleep(0.06)
            snap = app.snapshot(source="paint")
            framed = assert_focus_ring_concentric(snap)
            if framed is not None:
                rings += 1
        return rings, len(order)


def _sweep_scrolled_listbox() -> int:
    """Explicit non-zero-offset case: scroll the listbox, focus a row
    inside the now-visible window, assert the ring frames that row at its
    scroll-translated absolute position. This is the exact case the
    pre-R705.1 bug mis-drew."""
    checks = 0
    with RpcSubprocess("hello-listbox", boot_grace=1.0) as lb:
        lb.request("focus/set", {"tag": "main_list"})
        lb.request("scene/key", {"path": "main_list_scroll", "key": "PageDown"})
        time.sleep(0.1)
        # focus a row within the scrolled-into-view window
        lb.request("scene/rewind", {"path": "/external/focused_index", "value": 5})
        time.sleep(0.2)
        snap = lb.snapshot(source="paint")
        framed = assert_focus_ring_concentric(snap)
        assert framed is not None, "scrolled listbox row must carry a ring"
        assert framed.startswith("main_list#"), f"framed wrong node: {framed}"
        checks += 1
    return checks


def body() -> None:
    total_rings = 0
    total_stops = 0
    for example in GUI_EXAMPLES:
        try:
            rings, stops = _sweep_example(example)
        except RpcError as exc:
            raise AssertionError(f"{example}: RPC failure during sweep: {exc}") from exc
        total_rings += rings
        total_stops += stops
        print(f"  {example}: {rings} ring(s) verified across {stops} tab stop(s)")

    scrolled = _sweep_scrolled_listbox()
    print(f"  scrolled-listbox case: {scrolled} ring verified at non-zero offset")

    assert_eq(scrolled, 1, "scrolled listbox ring verified")
    # Every drawn ring across the catalogue was grounded against a real
    # node's window-absolute rect. We expect a healthy number of rings.
    assert total_rings >= 10, (
        f"expected >= 10 grounded rings across the catalogue, got {total_rings}"
    )
    print(
        f"  TOTAL: {total_rings} focus rings verified concentric "
        f"across {len(GUI_EXAMPLES)} examples ({total_stops} tab stops)"
    )


if __name__ == "__main__":
    sys.exit(run_demo("R705.1 focus-ring placement (grounded, all examples)", body))
