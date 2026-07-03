#!/usr/bin/env python3
"""R1167 §5.51 §2 #7 — SAME-window OUTER full-span dock (dock SSOT debt B).

SCOPE — read this before the assertions. `resolve_drop` always HANDLED an
`OuterDock` (a full-span perimeter dock), but only the CROSS-window input path
(`resolve_outer_dock_zone`) ever PRODUCED the outer sentinel — a same-window drag
could only inner-split the panel under the cursor, never reach the window-edge
full-span dock. That asymmetry was the structural root of the user's live
complaints ("console을 하단에 = full-width 안 됨", "properties를 오른쪽에 = 자기
컬럼 안 됨"). R1167 makes the resolver SYMMETRIC: a same-window dock-panel drag
whose cursor enters the window's outer band resolves to `OuterDock` too
(`InputRouter::resolve_own_outer_dock`, gated on the dock-panel kind so a non-dock
drag near the edge is unaffected). The PREVIEW and the RESULT both flow from that
one resolver, so the previewed full-span band == where the panel lands.

This drives the new path with a real same-window `scene/drag` on a panel header
and observes the §2 #7 scene-as-data:

  (A) Boot — 5 panels, outliner a narrow column, no reorg splits.
  (B) HELD drag to the BOTTOM outer band → the dragged panel's `drop_preview`
      shows the OUTER sentinel target + the Bottom edge (the same-window outer
      PREVIEW — could not exist before R1167).
  (C) Release → the outliner is now a FULL-WIDTH bottom row (the RESULT == the
      preview: preview == result, by construction).
  (D) A second edge — a same-window drag of properties to the RIGHT band makes it
      a FULL-HEIGHT right column.
  (E) Interior contrast — a held drag to a panel's CENTRE previews an INNER dock
      (target is a panel id, NOT the outer sentinel): the override fires ONLY in
      the edge band, so an interior drag still inner-docks.
  (F) Integrity — all 5 panels survive every move; reads are deterministic.

The live FEEL (the band width, drag smoothness) is HW-gated (a real `:0` desktop);
this pins what is observable as scene-as-data.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo, wait_until  # noqa: E402

_MAIN = "main"
_MAIN_W = 1200
_MAIN_H = 800
_REORG = "/dock_reorganize/external"

_TOOLBAR = "toolbar"
_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_ALL_PANELS = {_TOOLBAR, _OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}


# ─── helpers ─────────────────────────────────────────────────────────


def _is_outer(tag: Any) -> bool:
    # OUTER_DOCK_ZONE_TAG is a NUL-sentinel + "outer-dock-zone"; match by suffix
    # so the demo never has to embed a NUL byte.
    return isinstance(tag, str) and tag.endswith("outer-dock-zone")


def _topology(tf: RpcSubprocess) -> Any:
    return tf.query(f"{_REORG}/topology")


def _split_ids(node: Any) -> list[str]:
    if not isinstance(node, dict):
        return []
    if node.get("type") == "Split":
        return [node["id"]] + _split_ids(node.get("first")) + _split_ids(node.get("second"))
    return []


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in (tf.request("scene/windows", {}).result.get("windows") or [])}


def _all_panels(node: Any) -> set[str]:
    if not isinstance(node, dict):
        return set()
    kind = node.get("type")
    if kind == "Leaf":
        return {node.get("panel_id")}
    if kind == "Tabs":
        return set(node.get("panels") or [])
    if kind == "Split":
        return _all_panels(node.get("first")) | _all_panels(node.get("second"))
    return set()


def _panel_set(tf: RpcSubprocess) -> set[str]:
    return _all_panels(_topology(tf).get("root"))


def _layout(tf: RpcSubprocess) -> Any:
    resp = tf.request("scene/layout", {"viewport": {"width": _MAIN_W, "height": _MAIN_H}})
    assert resp is not None and resp.result is not None, "scene/layout must answer"
    return resp.result


def _find_rect(layout: Any, tag: str) -> Optional[dict[str, float]]:
    def walk(node: Any) -> Optional[dict[str, Any]]:
        if not isinstance(node, dict):
            return None
        if node.get("tag") == tag:
            r = node.get("rect")
            if isinstance(r, dict) and r.get("w", 0) > 0:
                return r
        for child in node.get("children") or []:
            found = walk(child)
            if found is not None:
                return found
        content = node.get("content")
        return walk(content) if isinstance(content, dict) else None

    rect = walk(layout)
    if not isinstance(rect, dict):
        return None
    return {k: float(rect.get(k, 0)) for k in ("x", "y", "w", "h")}


def _rect(tf: RpcSubprocess, tag: str) -> dict[str, float]:
    rect = _find_rect(_layout(tf), tag)
    assert rect is not None, f"{tag} rect must be present in scene/layout"
    return rect


def _drop_preview(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/drop_preview")


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        _section("A: boot — 5 panels, outliner a narrow column")
        wins = tf.request("scene/windows", {}).result.get("windows") or []
        assert len(wins) == 1, f"A.1 one window at boot; got {wins!r}"
        assert wins[0].get("id") == _MAIN, f"A.2 the window is main ({wins[0]!r})"
        assert _panel_set(tf) == _ALL_PANELS, f"A.3 all 5 panels present ({_panel_set(tf)})"
        outliner0 = _rect(tf, _OUTLINER)
        assert outliner0["w"] < 400, f"A.4 outliner starts narrow (w={outliner0['w']})"
        assert outliner0["h"] > 400, f"A.5 outliner starts tall (a column, h={outliner0['h']})"
        boot_reorg = [s for s in _split_ids(_topology(tf).get("root")) if s.startswith("reorg-")]
        assert boot_reorg == [], f"A.6 no reorg dividers at boot ({boot_reorg})"

        # ── (B) held drag to the BOTTOM outer band → OUTER preview ───
        _section("B: held drag to the bottom outer band → the OUTER preview")
        # main is 1200x800; the outer band is 32px. (600, 790) is 10px above the
        # bottom edge — inside the band. Hold the drag so the preview is readable.
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="begin")
        prev = _drop_preview(tf, _OUTLINER)
        assert isinstance(prev, dict), f"B.1 a held drag in the band has a preview ({prev!r})"
        assert prev.get("source") == _OUTLINER, f"B.2 the preview source is the dragged panel ({prev!r})"
        assert _is_outer(prev.get("target")), f"B.3 ★the target is the OUTER sentinel ({prev.get('target')!r})"
        assert prev.get("zone") == "Bottom", f"B.4 the previewed edge is Bottom ({prev.get('zone')!r})"
        # The outer dock is NOT a float: the panel stays docked + single-window
        # while held (a drag-out-to-float would have torn a floater off).
        assert _window_ids(tf) == {_MAIN}, "B.5 the held outer drag spawns no floater (not a float)"
        assert _OUTLINER in _panel_set(tf), "B.6 the outliner stays in the topology mid-drag"

        # ── (C) release → full-width bottom row (preview == result) ──
        _section("C: release → the outliner is a FULL-WIDTH bottom row")
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="end")
        wait_until(
            lambda: _rect(tf, _OUTLINER)["w"] >= _MAIN_W - 8,
            desc="C.1 the outliner now spans the full width",
        )
        after = _rect(tf, _OUTLINER)
        assert after["x"] <= 1.0, f"C.2 the outliner row starts at the left edge (x={after['x']})"
        assert after["w"] >= _MAIN_W - 8, f"C.3 ★it spans the full window width (w={after['w']})"
        assert after["y"] >= _MAIN_H * 0.5, f"C.4 it is the BOTTOM row (y={after['y']})"
        assert after["h"] < 260, f"C.5 a thin accessory band, not a half-window (h={after['h']})"
        assert _panel_set(tf) == _ALL_PANELS, "C.6 the outer dock is a move — no panel lost"
        assert _window_ids(tf) == {_MAIN}, "C.7 still one window (the outer dock is in-place, not a float)"

        # ── (D) a second edge — properties to the RIGHT band ─────────
        _section("D: a same-window drag of properties to the RIGHT band → right column")
        # (1190, 400) is 10px inside the right edge — hold to read the Right preview.
        tf.drag(from_path=f"{_PROPERTIES}#header", to_at=(1190.0, 400.0), phase="begin")
        dprev = _drop_preview(tf, _PROPERTIES)
        assert isinstance(dprev, dict) and _is_outer(dprev.get("target")), (
            f"D.1 the right-band held preview is OUTER ({dprev!r})"
        )
        assert dprev.get("zone") == "Right", f"D.2 the previewed edge is Right ({dprev.get('zone')!r})"
        tf.drag(from_path=f"{_PROPERTIES}#header", to_at=(1190.0, 400.0), phase="end")
        wait_until(
            lambda: _rect(tf, _PROPERTIES)["h"] >= 600,
            desc="D.3 properties now spans the full height",
        )
        props = _rect(tf, _PROPERTIES)
        assert props["x"] + props["w"] >= _MAIN_W - 8, f"D.4 properties is flush right ({props!r})"
        assert props["h"] >= 600, f"D.5 ★it spans the full window height (h={props['h']})"
        assert props["w"] < 360, f"D.6 a thin right column, not a half-window (w={props['w']})"
        assert _panel_set(tf) == _ALL_PANELS, "D.7 still a move — no panel lost"

        # ── (E) interior contrast — the override is edge-band only ───
        # (R1201) Drag CONSOLE, a movable panel: the toolbar was locked
        # non-movable at R1172 (`with_movable(false)`), so `toolbar#header` no
        # longer emits a drag handle — the pre-R1201 source here could not start a
        # drag at all. Any movable panel exercises the same edge-only override.
        _section("E: a held drag to a panel CENTRE previews an INNER dock, not outer")
        vp = _rect(tf, _VIEWPORT)
        cx, cy = vp["x"] + vp["w"] * 0.5, vp["y"] + vp["h"] * 0.5
        tf.drag(from_path=f"{_CONSOLE}#header", to_at=(cx, cy), phase="begin")
        inner = _drop_preview(tf, _CONSOLE)
        assert isinstance(inner, dict), f"E.1 an interior held drag has a preview ({inner!r})"
        assert not _is_outer(inner.get("target")), (
            f"E.2 ★the interior preview is NOT the outer sentinel — the override is edge-only "
            f"({inner.get('target')!r})"
        )
        assert inner.get("target") == _VIEWPORT, f"E.3 the inner target is the panel under the cursor ({inner!r})"
        assert inner.get("zone") == "Center", f"E.4 a panel-centre drop is a Center (tabify) zone ({inner!r})"
        tf.drag(from_path=f"{_CONSOLE}#header", to_at=(cx, cy), phase="end")

        # ── (F) integrity + determinism ──────────────────────────────
        _section("F: integrity — panels survive, reads deterministic")
        assert _window_ids(tf) == {_MAIN}, "F.1 only main remains (no spurious floater)"
        assert _panel_set(tf) == _ALL_PANELS, "F.2 all 5 panels intact after every move"
        a = _topology(tf)
        b = _topology(tf)
        assert a == b, "F.3 back-to-back topology reads are identical"

        print("[demo] r1167_same_window_outer_dock: all sections PASS (same-window outer dock)")


if __name__ == "__main__":
    sys.exit(run_demo("r1167_same_window_outer_dock", body))
