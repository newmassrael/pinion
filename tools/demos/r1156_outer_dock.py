#!/usr/bin/env python3
"""R1156 §5.51 §2 #7 — OUTER full-span dock: a drop at the dock area's PERIMETER
docks across EVERY pane (a full-width row / full-height column), not into one
panel.

SCOPE — read this before the assertions. The dock laid panels out in columns
(outliner | viewport | properties); the per-panel drop zones could only split ONE
panel, so a torn-off panel could never return as the full-width top row it came
from. R1156 adds OUTER dock zones at the dock area's perimeter: the cross-window
resolution (`scene/cross_window_drop`, the §2 #7 introspection peer of the live
redock) returns the reserved outer-dock sentinel tag when the cursor is in the
outermost band, and `DockReorganizer::split_root` wraps the WHOLE tree so the new
panel spans every pane. This demo pins both, headless:

  (A) boot — one 'main' window.
  (B) PERIMETER -> outer — a cursor in the top/bottom/left/right margin band of the
      window content resolves to the outer-dock sentinel tag (the full-span dock),
      across many sample points; the INTERIOR resolves an inner panel instead.
  (C) the AI `dock_outer` invoke moves a narrow COLUMN panel (outliner, ~215px) to
      a FULL-SPAN top row (full window width) — and properties to a full-height
      left column; Center is rejected (the area's centre is not an outer dock).

The live FEEL (dragging a floater to the edge, the full-span preview) is HW-gated
(a real `:0` desktop); this pins what is observable as scene-as-data.
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

_MAIN = "main"
_REORG = "dock_reorganize"


def _is_outer(tag) -> bool:
    # OUTER_DOCK_ZONE_TAG is a NUL-sentinel + "outer-dock-zone"; match by suffix
    # so the test never has to embed a NUL byte.
    return isinstance(tag, str) and tag.endswith("outer-dock-zone")


def _drop_tag(tf: RpcSubprocess, x: float, y: float):
    resp = tf.request("scene/cross_window_drop", {"x": float(x), "y": float(y)})
    assert resp is not None and resp.result is not None, "scene/cross_window_drop must answer"
    drop = resp.result.get("drop")
    return drop.get("tag") if drop else None


def _rect_of(tf: RpcSubprocess, tag: str):
    tree = tf.request("scene/snapshot", {"path": "/window[main]", "from": "paint"}).result
    found = {}

    def walk(n):
        if isinstance(n, dict):
            if n.get("tag") == tag and n.get("rect", {}).get("w", 0) > 0:
                found["r"] = n["rect"]
            for c in n.get("children") or []:
                walk(c)
            if isinstance(n.get("content"), dict):
                walk(n["content"])

    walk(tree)
    return found.get("r")


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        _section("A: boot — one main window")
        wins = tf.request("scene/windows", {}).result.get("windows") or []
        assert len(wins) == 1, f"A.1 one window; got {wins!r}"
        assert wins[0].get("id") == _MAIN, f"A.2 the window is main; got {wins[0]!r}"

        _section("B: PERIMETER resolves to the OUTER full-span zone; interior to a panel")
        # main is 1200x800; the outer band is 32px. Sample each edge.
        perimeter = [
            (120, 6, "top"), (400, 20, "top"), (900, 30, "top"),
            (6, 200, "left"), (20, 600, "left"),
            (1194, 200, "right"), (1180, 600, "right"),
            (300, 794, "bottom"), (800, 780, "bottom"),
        ]
        for x, y, lbl in perimeter:
            tag = _drop_tag(tf, x, y)
            assert _is_outer(tag), f"B {lbl} ({x},{y}) is the OUTER zone; got {tag!r}"
        # Interior (well clear of every edge) resolves an inner panel, not outer.
        for x, y, lbl in [(600, 400, "centre"), (300, 400, "left col"), (1050, 400, "right col")]:
            tag = _drop_tag(tf, x, y)
            assert tag is not None and not _is_outer(tag), f"B interior {lbl} is a panel; got {tag!r}"

        _section("C: dock_outer invoke spans a narrow column FULL-WIDTH / FULL-HEIGHT")
        win_w = 1200
        # Top: outliner (a ~215px column) becomes a full-width top row.
        before = _rect_of(tf, "outliner")
        assert before is not None and before["w"] < 400, f"C.1 outliner starts narrow ({before!r})"
        out = tf.invoke(f"/{_REORG}/external/dock_outer", {"source": "outliner", "zone": "Top"})
        assert out == "outliner -> outer Top", f"C.2 invoke outcome ({out!r})"
        after = _rect_of(tf, "outliner")
        assert after is not None, "C.3 outliner still present after the outer dock"
        assert after["w"] >= win_w - 8, f"C.4 outliner now spans the full width ({after!r})"
        # (R1206) The outer-Top dock splits the WORKSPACE (below the fixed toolbar
        # frame), so the top row sits at the workspace top (~48px), not y=0 — the
        # outer dock no longer reaches the toolbar.
        assert after["x"] == 0 and 40 <= after["y"] <= 56, f"C.5 it is the workspace TOP row ({after!r})"

        # Left: properties becomes a full-height left column.
        out = tf.invoke(f"/{_REORG}/external/dock_outer", {"source": "properties", "zone": "Left"})
        assert out == "properties -> outer Left", f"C.6 invoke outcome ({out!r})"
        props = _rect_of(tf, "properties")
        assert props is not None and props["x"] == 0, f"C.7 properties is the LEFT column ({props!r})"
        assert props["h"] >= 600, f"C.8 properties spans the full height ({props!r})"

        # Centre is not an outer dock — rejected.
        out = tf.invoke(f"/{_REORG}/external/dock_outer", {"source": "console", "zone": "Center"})
        assert out is None or "edge zone" in str(out), f"C.9 centre is not an outer dock ({out!r})"

        print("[demo] r1156_outer_dock: all sections PASS (full-span outer dock)")


if __name__ == "__main__":
    sys.exit(run_demo("r1156_outer_dock", body))
