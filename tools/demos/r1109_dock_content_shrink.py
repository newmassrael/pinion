#!/usr/bin/env python3
"""R1109 §5.21 PR-35 — dock panel content shrinks below its intrinsic height.

Drives `hello-dock-panels-editor` (the R685 5-pane editor). The console
pane (R1109: a realistic 28-line scrollback, far taller than the pane it
sits in) is the forcing consumer for the R1086 flex-main idiom applied to
`view_dock_panel`'s content wrapper in
`crates/pinion-widget-paint/src/dock.rs`.

Pre-R1109 the content wrapper carried `with_flex_grow(1.0)` alone — the
incomplete half of the idiom. Taffy's CSS automatic flex minimum then pinned
the wrapper to its content's intrinsic min-content height, so a pane whose
content is taller than its allotted height (the console scrollback) clamped to
content and **overflowed** the pane: the wrapper rect extended hundreds of px
below the panel root. The full `flex_basis:0 + flex_grow:1 + min-height:0`
idiom lets the wrapper shrink to the panel's leftover height (header excluded)
while the cross axis still stretches to the panel width.

Everything is observed over the §2 #2 RPC-as-primary-path contract:
`scene/snapshot source=paint` yields the laid-out scene with per-node rects;
the demo never re-derives layout.

Section roadmap (>=30 assertions across A-F):

  (A) Snapshot sanity — paint root is the app-frame Column Container.
  (B) 4-panel workspace — every panel root + its `#header` + `#content`
      wrapper is present in the laid-out scene.
  (C) Console forcing consumer — the content wrapper fits WITHIN the
      console panel (`wrapper.h <= panel.h`, the invariant that fails
      pre-R1109 when content overflows), exactly fills the leftover
      `panel.h - header.h`, never extends past the panel's bottom, and is
      strictly shorter than the scrollback's intrinsic stack height (so the
      shrink is meaningful, not a small-content no-op).
  (D) Editor unaffected (small content) — for EVERY pane the wrapper still
      fills `panel.h - header.h` and stretches to the panel width; the
      R1086 idiom is behaviour-preserving when content fits.
  (E) Determinism — two back-to-back snapshots report identical console
      rects.
  (F) Cross-axis — the console wrapper stretches to the full panel width
      (AlignItems::Stretch is untouched on the cross axis).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

# ─── constants mirrored from the binding / substrate ────────────────

_MAIN_W = 1200
_MAIN_H = 800

# (R1206) The toolbar is a fixed frame outside the dock, not a dock panel — the
# workspace is these 4 panels, each with a `#header` + `#content`.
_PANEL_TAGS = ("outliner", "viewport", "properties", "console")
_CONSOLE_PANEL_TAG = "console"

# `view_dock_panel` composite tags: `<panel>#header` / `<panel>#content`
# (HEADER_TAG_SUFFIX / CONTENT_TAG_SUFFIX in dock.rs).
_HEADER_SUFFIX = "header"
_CONTENT_SUFFIX = "content"

# Console scrollback line count (mirrors CONSOLE_ROWS in the editor binding).
_CONSOLE_ROW_COUNT = 28
# A conservative lower bound on a single console row's laid-out height
# (13px font + 2+2 px row padding). The intrinsic scrollback stack is at
# least `_CONSOLE_ROW_COUNT * _MIN_ROW_PX` tall; only the shrink fix keeps
# the wrapper well below that.
_MIN_ROW_PX = 14


# ─── snapshot + scene-walk helpers ──────────────────────────────────


def _snapshot(tf: RpcSubprocess) -> Any:
    return tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H))


def _find(scene: Any, tag: str) -> Optional[dict]:
    """Depth-first search for the first node whose `tag` field == `tag`."""
    if not isinstance(scene, dict):
        return None
    if scene.get("tag") == tag:
        return scene
    for child in scene.get("children") or []:
        hit = _find(child, tag)
        if hit is not None:
            return hit
    content = scene.get("content")
    if isinstance(content, dict):
        return _find(content, tag)
    return None


def _rect(node: dict) -> dict:
    rect = node.get("rect")
    assert isinstance(rect, dict), f"node {node.get('tag')!r} carries a rect"
    return rect


def _composite(panel: str, suffix: str) -> str:
    return f"{panel}#{suffix}"


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ──────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor") as tf:
        # ─── (A) Snapshot sanity ─────────────────────────────────────
        _section("A: snapshot sanity")
        scene = _snapshot(tf)
        assert scene is not None, "A.1 paint snapshot returns non-None"
        assert isinstance(scene, dict), "A.2 snapshot is a dict"
        assert scene.get("type") == "Container", "A.3 paint root is a Container"

        # ─── (B) 4-panel workspace ────────────────────────────────────
        _section("B: every pane has a header + content wrapper")
        for panel in _PANEL_TAGS:
            assert _find(scene, panel) is not None, f"B.{panel}.root panel root present"
            assert (
                _find(scene, _composite(panel, _HEADER_SUFFIX)) is not None
            ), f"B.{panel}.header header strip present"
            assert (
                _find(scene, _composite(panel, _CONTENT_SUFFIX)) is not None
            ), f"B.{panel}.content content wrapper present"

        # ─── (C) Console forcing consumer ────────────────────────────
        _section("C: console content shrinks to fit its pane")
        console = _find(scene, _CONSOLE_PANEL_TAG)
        assert console is not None, "C.1 console panel root present"
        c_rect = _rect(console)
        c_h = int(c_rect.get("h", 0))
        assert c_h > 0, f"C.2 console panel root has positive height ({c_h})"

        c_header = _find(scene, _composite(_CONSOLE_PANEL_TAG, _HEADER_SUFFIX))
        assert c_header is not None, "C.3 console header present"
        header_h = int(_rect(c_header).get("h", 0))
        assert header_h > 0, f"C.4 console header has positive height ({header_h})"

        c_content = _find(scene, _composite(_CONSOLE_PANEL_TAG, _CONTENT_SUFFIX))
        assert c_content is not None, "C.5 console content wrapper present"
        content_rect = _rect(c_content)
        content_h = int(content_rect.get("h", 0))
        content_y = int(content_rect.get("y", 0))

        # The decisive invariant: the content wrapper fits WITHIN the panel.
        # Pre-R1109 it clamped to the ~600px scrollback and overflowed.
        assert content_h <= c_h, (
            f"C.6 console content wrapper ({content_h}px) must fit within its "
            f"panel ({c_h}px) - pre-R1109 it overflowed to the scrollback's "
            f"intrinsic height"
        )
        # It exactly fills the leftover space after the fixed-height header.
        assert abs(content_h - (c_h - header_h)) <= 1, (
            f"C.7 console content wrapper ({content_h}px) fills the pane's "
            f"leftover height ({c_h} - {header_h} = {c_h - header_h}px)"
        )
        # And never extends past the panel's bottom edge.
        c_bottom = int(c_rect.get("y", 0)) + c_h
        content_bottom = content_y + content_h
        assert content_bottom <= c_bottom + 1, (
            f"C.8 console content bottom ({content_bottom}) stays within the "
            f"panel bottom ({c_bottom}) - no overflow"
        )
        # The shrink is meaningful: the scrollback's intrinsic stack is far
        # taller than the pane, so the wrapper is genuinely clipped.
        intrinsic_floor = _CONSOLE_ROW_COUNT * _MIN_ROW_PX
        assert content_h < intrinsic_floor, (
            f"C.9 console wrapper ({content_h}px) is shorter than the "
            f"{_CONSOLE_ROW_COUNT}-row scrollback's intrinsic floor "
            f"({intrinsic_floor}px) - the fix is exercised, not a no-op"
        )
        assert c_h < intrinsic_floor, (
            f"C.10 console pane ({c_h}px) is itself shorter than the "
            f"scrollback floor ({intrinsic_floor}px) - a real overflow case"
        )

        # ─── (D) Editor unaffected (small content) ───────────────────
        _section("D: every pane wrapper fills its pane (behaviour-preserving)")
        for panel in _PANEL_TAGS:
            root = _find(scene, panel)
            header = _find(scene, _composite(panel, _HEADER_SUFFIX))
            content = _find(scene, _composite(panel, _CONTENT_SUFFIX))
            assert root and header and content, f"D.{panel}.nodes resolved"
            ph = int(_rect(root).get("h", 0))
            hh = int(_rect(header).get("h", 0))
            cw_h = int(_rect(content).get("h", 0))
            assert abs(cw_h - (ph - hh)) <= 1, (
                f"D.{panel}.fills wrapper ({cw_h}px) == panel - header "
                f"({ph} - {hh} = {ph - hh}px)"
            )
            assert cw_h <= ph, f"D.{panel}.contained wrapper fits within panel"

        # ─── (E) Determinism ─────────────────────────────────────────
        _section("E: two snapshots report identical console rects")
        scene2 = _snapshot(tf)
        c_content2 = _find(scene2, _composite(_CONSOLE_PANEL_TAG, _CONTENT_SUFFIX))
        assert c_content2 is not None, "E.1 console wrapper present on re-query"
        rect2 = _rect(c_content2)
        assert int(rect2.get("h", 0)) == content_h, "E.2 wrapper height stable"
        assert int(rect2.get("w", -1)) == int(content_rect.get("w", -2)), (
            "E.3 wrapper width stable"
        )

        # ─── (F) Cross-axis stretch ──────────────────────────────────
        _section("F: console wrapper stretches to full panel width")
        c_w = int(c_rect.get("w", 0))
        content_w = int(content_rect.get("w", 0))
        assert c_w > 0, f"F.1 console panel has positive width ({c_w})"
        assert content_w == c_w, (
            f"F.2 console content wrapper width ({content_w}px) == panel width "
            f"({c_w}px) - AlignItems::Stretch fills the cross axis"
        )


if __name__ == "__main__":
    sys.exit(run_demo("r1109_dock_content_shrink", body))
