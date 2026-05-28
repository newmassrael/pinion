#!/usr/bin/env python3
"""R686 §5.16 §5.45 — DockSurface drag-to-reorganize demo.

Drives `hello-dock-panels-editor` (the R685 5-pane editor) through the
R686 drag-to-reorganize substrate: the `DockReorganizeExternal`
registered at `/dock_reorganize`. The AI client reads `scene/layout`
for panel rects, classifies a drop position into a `DockDropZone` with
the same geometry the Rust `dock_drop_zone_for` uses, and applies the
gesture via `scene/invoke /dock_reorganize/external/reorganize` — the
§2 #2 RPC-as-primary-path contract. The topology mutates reactively
(the external `set`s the shared `Signal<DockTopology>`), so a follow-up
`scene/query /dock_reorganize/external/topology` + `scene/layout`
observe the rearranged layout.

Section roadmap (>=40 assertions across A-H):

  (A) Substrate sanity — the reorganize external exposes the live
      topology as JSON; split_seq starts at 0; 5 panels in canonical
      depth-first order.
  (B) Baseline layout — every panel root rect is present + non-
      degenerate in `scene/layout`.
  (C) Drop-zone classification — the Python mirror of
      `dock_drop_zone_for` classifies a panel rect's centre / edges /
      exterior the way the Rust unit tests pin (centre=Center,
      near-edge=directional, outside=None).
  (D) Centre drop = swap — classify the centre of one panel, invoke a
      Center reorganize, confirm the two panels traded depth-first
      slots + split_seq stayed 0 (a swap mints no divider).
  (E) Edge drop = split-insert — classify a panel's left edge, invoke
      a Left reorganize moving another panel beside it; confirm a
      `reorg-split-N` divider appeared, leaf count held at 5 (a move,
      not a spawn), and split_seq bumped to 1.
  (F) Layout reflow — re-query `scene/layout`; the relocated panel now
      sits left of its new neighbour (docked-left geometry).
  (G) Rejected gestures — a stale source id + an unknown zone both
      reject; the live topology is unchanged; a self-drop is a
      well-defined identity no-op.
  (H) Determinism — two back-to-back topology queries are identical.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo  # noqa: E402

# ─── constants mirrored from the binding / substrate ────────────────

_MAIN_W = 1200
_MAIN_H = 800
_SETTLE_SEC = 0.15

# Panel root tags (DockPanelStyle::m3_default(panel_id) → tag == id).
_TOOLBAR = "toolbar"
_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_CANONICAL_PANELS = [_TOOLBAR, _OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE]

# Reorganize external + its minted-split prefix (mirror of
# `REORG_SPLIT_ID_PREFIX` + `DOCK_REORGANIZE_TAG`).
_REORG_TAG = "dock_reorganize"
_REORG_SPLIT_PREFIX = "reorg-split-"

# Edge-band fraction (mirror of `DOCK_EDGE_ZONE_FRAC`).
_EDGE_FRAC = 0.25


# ─── topology JSON walkers ──────────────────────────────────────────


def _panel_ids(topo: Any) -> list[str]:
    """Depth-first leaf panel ids from a topology JSON ({"root": ...})."""
    out: list[str] = []

    def walk(node: Any) -> None:
        if not isinstance(node, dict):
            return
        if node.get("type") == "Leaf":
            out.append(node["panel_id"])
        elif node.get("type") == "Split":
            walk(node["first"])
            walk(node["second"])

    walk(topo.get("root"))
    return out


def _split_ids(topo: Any) -> list[str]:
    """Depth-first pre-order split ids from a topology JSON."""
    out: list[str] = []

    def walk(node: Any) -> None:
        if not isinstance(node, dict):
            return
        if node.get("type") == "Split":
            out.append(node["id"])
            walk(node["first"])
            walk(node["second"])

    walk(topo.get("root"))
    return out


def _topology(tf: RpcSubprocess) -> Any:
    return tf.query(f"/{_REORG_TAG}/external/topology")


def _split_seq(tf: RpcSubprocess) -> int:
    return int(tf.query(f"/{_REORG_TAG}/external/split_seq"))


def _reorganize(tf: RpcSubprocess, source: str, target: str, zone: str) -> Any:
    return tf.invoke(
        f"/{_REORG_TAG}/external/reorganize",
        {"source": source, "target": target, "zone": zone},
    )


# ─── layout walkers + drop-zone mirror ──────────────────────────────


def _layout(tf: RpcSubprocess) -> Any:
    resp = tf.request(
        "scene/layout",
        {"viewport": {"width": _MAIN_W, "height": _MAIN_H}},
    )
    assert resp is not None
    return resp.result


def _find_rect(layout: Any, tag: str) -> Optional[dict[str, float]]:
    """Walk `scene/layout` for the rect of the node tagged `tag`."""

    def walk(node: Any) -> Optional[dict[str, Any]]:
        if not isinstance(node, dict):
            return None
        if node.get("tag") == tag:
            return node.get("rect")
        for child in node.get("children") or []:
            found = walk(child)
            if found is not None:
                return found
        content = node.get("content")
        if isinstance(content, dict):
            return walk(content)
        return None

    rect = walk(layout)
    if not isinstance(rect, dict):
        return None
    return {k: float(rect.get(k, 0)) for k in ("x", "y", "w", "h")}


def _drop_zone(rect: dict[str, float], x: float, y: float) -> str:
    """Python mirror of `dock_drop_zone_for` (Rust is the SoT; the unit
    tests in dock.rs pin the canonical behaviour — this replica keeps
    the demo honest about driving the same geometry)."""
    rx, ry, rw, rh = rect["x"], rect["y"], rect["w"], rect["h"]
    if rw == 0 or rh == 0:
        return "None"
    if x < rx or y < ry or x >= rx + rw or y >= ry + rh:
        return "None"
    from_left = (x - rx) / rw
    from_right = 1.0 - from_left
    from_top = (y - ry) / rh
    from_bottom = 1.0 - from_top
    nearest = min(from_left, from_right, from_top, from_bottom)
    if nearest >= _EDGE_FRAC:
        return "Center"
    if from_left <= from_right and from_left <= from_top and from_left <= from_bottom:
        return "Left"
    if from_right <= from_top and from_right <= from_bottom:
        return "Right"
    if from_top <= from_bottom:
        return "Top"
    return "Bottom"


def _center(rect: dict[str, float]) -> tuple[float, float]:
    return rect["x"] + rect["w"] * 0.5, rect["y"] + rect["h"] * 0.5


def _left_edge(rect: dict[str, float]) -> tuple[float, float]:
    return rect["x"] + rect["w"] * 0.05, rect["y"] + rect["h"] * 0.5


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ──────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor") as tf:
        time.sleep(_SETTLE_SEC)

        # ─── (A) substrate sanity ────────────────────────────────────
        _section("A: reorganize external sanity")
        topo = _topology(tf)
        assert isinstance(topo, dict), "A.1 topology query returns JSON object"
        assert "root" in topo, "A.2 topology JSON exposes the root node"
        assert _panel_ids(topo) == _CANONICAL_PANELS, (
            f"A.3 canonical depth-first panel order, got {_panel_ids(topo)}"
        )
        assert len(_split_ids(topo)) == 4, "A.4 four boot splits"
        assert _split_seq(tf) == 0, "A.5 split_seq starts at 0"

        # ─── (B) baseline layout ─────────────────────────────────────
        _section("B: baseline panel rects")
        layout = _layout(tf)
        rects: dict[str, dict[str, float]] = {}
        for panel in _CANONICAL_PANELS:
            rect = _find_rect(layout, panel)
            assert rect is not None, f"B.{panel} rect present in scene/layout"
            assert rect["w"] > 0 and rect["h"] > 0, (
                f"B.{panel} rect non-degenerate ({rect})"
            )
            rects[panel] = rect
        # The editor's middle row tiles outliner | viewport | properties
        # left-to-right, so their x origins are strictly increasing.
        assert rects[_OUTLINER]["x"] < rects[_VIEWPORT]["x"], (
            "B.order outliner left of viewport"
        )
        assert rects[_VIEWPORT]["x"] < rects[_PROPERTIES]["x"], (
            "B.order viewport left of properties"
        )

        # ─── (C) drop-zone classification ────────────────────────────
        _section("C: drop-zone geometry mirror")
        prop = rects[_PROPERTIES]
        cx, cy = _center(prop)
        assert _drop_zone(prop, cx, cy) == "Center", "C.1 centre is Center"
        lx, ly = _left_edge(prop)
        assert _drop_zone(prop, lx, ly) == "Left", "C.2 left edge is Left"
        assert (
            _drop_zone(prop, cx, prop["y"] + prop["h"] * 0.02) == "Top"
        ), "C.3 top edge is Top"
        assert (
            _drop_zone(prop, prop["x"] + prop["w"] * 0.98, cy) == "Right"
        ), "C.4 right edge is Right"
        assert _drop_zone(prop, prop["x"] - 5.0, cy) == "None", (
            "C.5 outside-left is None"
        )
        assert (
            _drop_zone(prop, prop["x"] + prop["w"], cy) == "None"
        ), "C.6 exclusive right edge is None (half-open)"

        # ─── (D) centre drop = swap ──────────────────────────────────
        _section("D: centre drop swaps panels")
        # Classify the centre of "properties", then swap outliner onto it.
        zone = _drop_zone(prop, *_center(prop))
        assert zone == "Center", "D.1 target centre classifies as Center"
        outcome = _reorganize(tf, _OUTLINER, _PROPERTIES, zone)
        assert isinstance(outcome, str) and "outliner" in outcome, (
            f"D.2 swap returns an outcome summary ({outcome})"
        )
        topo = _topology(tf)
        after = _panel_ids(topo)
        assert after == [_TOOLBAR, _PROPERTIES, _VIEWPORT, _OUTLINER, _CONSOLE], (
            f"D.3 outliner<->properties swapped, got {after}"
        )
        assert _split_ids(topo) == [
            "editor_split_outer",
            "editor_split_inner_v",
            "editor_split_middle_h",
            "editor_split_inner_h",
        ], "D.4 swap leaves the split tree shape intact"
        assert _split_seq(tf) == 0, "D.5 a swap mints no divider (seq unchanged)"

        # ─── (E) edge drop = split-insert ────────────────────────────
        _section("E: edge drop docks a panel beside another")
        layout = _layout(tf)
        viewport_rect = _find_rect(layout, _VIEWPORT)
        assert viewport_rect is not None, "E.1 viewport rect present post-swap"
        lx, ly = _left_edge(viewport_rect)
        zone = _drop_zone(viewport_rect, lx, ly)
        assert zone == "Left", f"E.2 viewport left edge classifies Left ({zone})"
        outcome = _reorganize(tf, _CONSOLE, _VIEWPORT, zone)
        assert isinstance(outcome, str) and "console" in outcome, (
            f"E.3 split-insert returns an outcome ({outcome})"
        )
        topo = _topology(tf)
        after = _panel_ids(topo)
        assert len(after) == 5, f"E.4 leaf count held at 5 (a move, not a spawn): {after}"
        assert _CONSOLE in after, "E.5 console still present"
        # Left drop → console occupies the first slot beside viewport, so
        # console immediately precedes viewport in depth-first order.
        ci, vi = after.index(_CONSOLE), after.index(_VIEWPORT)
        assert ci + 1 == vi, f"E.6 console docked immediately left of viewport ({after})"
        new_splits = [s for s in _split_ids(topo) if s.startswith(_REORG_SPLIT_PREFIX)]
        assert new_splits == [f"{_REORG_SPLIT_PREFIX}0"], (
            f"E.7 exactly one reorg split minted, got {new_splits}"
        )
        assert _split_seq(tf) == 1, "E.8 split_seq bumped to 1"

        # ─── (F) layout reflow ───────────────────────────────────────
        _section("F: layout reflects the new docking")
        layout = _layout(tf)
        console_rect = _find_rect(layout, _CONSOLE)
        viewport_rect = _find_rect(layout, _VIEWPORT)
        assert console_rect is not None, "F.1 console rect present after reorg"
        assert viewport_rect is not None, "F.2 viewport rect present after reorg"
        assert console_rect["x"] < viewport_rect["x"], (
            f"F.3 console docked left of viewport "
            f"(console.x={console_rect['x']}, viewport.x={viewport_rect['x']})"
        )
        assert console_rect["w"] > 0 and console_rect["h"] > 0, (
            "F.4 relocated console rect non-degenerate"
        )

        # ─── (G) rejected + identity gestures ────────────────────────
        _section("G: rejected + identity gestures")
        before = _topology(tf)
        try:
            _reorganize(tf, "ghost", _VIEWPORT, "Center")
            raise AssertionError("G.1 stale source must reject")
        except RpcError as exc:
            assert exc.code != 0, f"G.1 stale source rejected (code {exc.code})"
        try:
            _reorganize(tf, _VIEWPORT, _PROPERTIES, "Diagonal")
            raise AssertionError("G.2 unknown zone must reject")
        except RpcError as exc:
            assert exc.code != 0, f"G.2 unknown zone rejected (code {exc.code})"
        assert _topology(tf) == before, "G.3 rejected gestures leave topology unchanged"
        # Self-drop (swap a panel with itself) is a well-defined identity.
        _reorganize(tf, _VIEWPORT, _VIEWPORT, "Center")
        assert _topology(tf) == before, "G.4 self-swap is an identity no-op"
        assert _split_seq(tf) == 1, "G.5 identity / rejected gestures mint no split"

        # ─── (H) determinism ─────────────────────────────────────────
        _section("H: query determinism")
        snap_a = _topology(tf)
        snap_b = _topology(tf)
        assert snap_a == snap_b, "H.1 back-to-back topology queries are identical"
        assert _panel_ids(snap_a) == _panel_ids(snap_b), "H.2 panel order stable"

        print("[demo] r686_dock_reorganize: all sections PASS")


if __name__ == "__main__":
    sys.exit(run_demo("r686_dock_reorganize", body))
