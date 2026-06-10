#!/usr/bin/env python3
"""R686 §5.16 §5.45 — DockSurface drag-to-reorganize demo.

Drives `hello-dock-panels-editor` (the R685 5-pane editor) through the
R687 drag-to-reorganize substrate: the `DockReorganizeExternal`
registered at `/dock_reorganize`, with its two wire forms:

  * `drop`  — geometry / cursor path. The client reads `scene/layout`
    for panel rects and hands the substrate the rects + the release
    cursor; the **substrate** classifies the drop zone
    (`dock_drop_zone_for`) and resolves the gesture
    (`resolve_dock_drop`). The demo never re-implements the zone
    geometry — the Rust helper is the single source of truth.
  * `reorganize` — symbolic path. An agent expresses the edit by panel
    ids + zone name (no pixels).

Both mutate the shared `Signal<DockTopology>` reactively, so a follow-up
`scene/query /dock_reorganize/external/topology` + `scene/layout`
observe the rearranged layout — the §2 #2 RPC-as-primary-path contract.

Section roadmap (>=40 assertions across A-H):

  (A) Substrate sanity — the reorganize external exposes the live
      topology as JSON; split_seq starts at 0; 5 panels in canonical
      depth-first order.
  (B) Baseline layout — every panel root rect is present + non-
      degenerate in `scene/layout`.
  (C) Substrate no-op drops — dropping onto the source itself or into
      dead space resolves to no target (Null), topology unchanged.
  (D) Centre drop = swap — `drop` with a cursor at a panel's centre;
      the substrate classifies Center + swaps; split_seq stays 0.
  (E) Edge drop = split-insert — `drop` with a cursor near a panel's
      left edge; the substrate classifies Left + docks the dragged
      panel beside it; a `reorg-split-N` divider appears, leaf count
      holds at 5 (a move), split_seq bumps to 1.
  (E2) Runtime registration (R688) — the `reorg-split-N` divider, which
      had no `SplitterExternal` at boot, now resolves + drags: R688's
      `CoreShell::reconcile_externals` registered it when the topology
      Signal changed. Dragging its handle moves the ratio Signal.
  (F) Layout reflow — re-query `scene/layout`; the relocated panel now
      sits left of its new neighbour (docked-left geometry).
  (G) Symbolic path + rejected gestures — the `reorganize` symbolic
      form swaps by id; a stale source + unknown zone + malformed
      payload all reject; the live topology is unchanged.
  (H) Determinism — two back-to-back topology queries are identical.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

# ─── constants mirrored from the binding / substrate ────────────────

_MAIN_W = 1200
_MAIN_H = 800

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
    """Symbolic reorganize — the AI-agent path (panel ids + zone name,
    no pixels). The substrate maps the zone to the topology edit."""
    return tf.invoke(
        f"/{_REORG_TAG}/external/reorganize",
        {"source": source, "target": target, "zone": zone},
    )


def _drop(tf: RpcSubprocess, source: str, cursor: tuple[float, float], panels: Any) -> Any:
    """Geometry / cursor path — hand the substrate the observed
    scene/layout rects + the release cursor; it classifies the drop
    zone itself (dock_drop_zone_for is the single source of truth, so
    this demo never re-implements the zone geometry)."""
    return tf.invoke(
        f"/{_REORG_TAG}/external/drop",
        {"source": source, "cursor": {"x": cursor[0], "y": cursor[1]}, "panels": panels},
    )


def _panels_payload(rects: dict[str, dict[str, float]]) -> list[dict[str, Any]]:
    """Build the `drop` action's panels payload from observed rects."""
    return [
        {"tag": tag, "rect": {k: int(r[k]) for k in ("x", "y", "w", "h")}}
        for tag, r in rects.items()
    ]


def _all_panel_rects(tf: RpcSubprocess) -> dict[str, dict[str, float]]:
    layout = _layout(tf)
    out: dict[str, dict[str, float]] = {}
    for panel in _CANONICAL_PANELS:
        rect = _find_rect(layout, panel)
        if rect is not None:
            out[panel] = rect
    return out


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


def _find_node(layout: Any, tag: str) -> Optional[dict[str, Any]]:
    """Walk `scene/layout` for the full node (rect + children) tagged
    `tag` — used to locate a splitter's handle child for a raw drag."""

    def walk(node: Any) -> Optional[dict[str, Any]]:
        if not isinstance(node, dict):
            return None
        if node.get("tag") == tag:
            return node
        for child in node.get("children") or []:
            found = walk(child)
            if found is not None:
                return found
        content = node.get("content")
        if isinstance(content, dict):
            return walk(content)
        return None

    return walk(layout)


def _splitter_ratio(tf: RpcSubprocess, tag: str) -> Optional[float]:
    val = tf.query(f"/{tag}/external/ratio")
    return float(val) if isinstance(val, (int, float)) else None


def _center(rect: dict[str, float]) -> tuple[float, float]:
    return rect["x"] + rect["w"] * 0.5, rect["y"] + rect["h"] * 0.5


def _left_edge(rect: dict[str, float]) -> tuple[float, float]:
    return rect["x"] + rect["w"] * 0.05, rect["y"] + rect["h"] * 0.5


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ──────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor") as tf:
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

        # ─── (C) substrate drop resolution — no-op cases ─────────────
        _section("C: drop over no target is a substrate no-op")
        panels = _panels_payload(rects)
        before_c = _topology(tf)
        # Drop a panel onto its own rect → resolve_dock_drop skips the
        # source → no target → Null no-op (the substrate decides, not us).
        res = _drop(tf, _PROPERTIES, _center(rects[_PROPERTIES]), panels)
        assert res is None, f"C.1 self-drop returns Null no-op, got {res!r}"
        assert _topology(tf) == before_c, "C.2 self-drop leaves topology unchanged"
        # Drop into dead space far outside every panel rect → no-op.
        res = _drop(tf, _OUTLINER, (float(_MAIN_W * 4), float(_MAIN_H * 4)), panels)
        assert res is None, "C.3 drop into empty space returns Null no-op"
        assert _topology(tf) == before_c, "C.4 empty-space drop unchanged"
        assert _split_seq(tf) == 0, "C.5 no-op drops mint no split"

        # ─── (D) centre drop = swap (substrate classifies) ───────────
        _section("D: centre drop swaps panels")
        # Hand the substrate the observed rects + a cursor at the centre
        # of "properties"; it classifies Center → swaps outliner onto it.
        outcome = _drop(tf, _OUTLINER, _center(rects[_PROPERTIES]), panels)
        assert isinstance(outcome, str), f"D.1 drop returns an outcome ({outcome!r})"
        assert "outliner" in outcome, f"D.2 outcome names the source ({outcome})"
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
        assert _split_seq(tf) == 0, "D.5 a centre drop is a swap — no divider minted"

        # ─── (E) edge drop = split-insert (substrate classifies) ─────
        _section("E: edge drop docks a panel beside another")
        rects = _all_panel_rects(tf)
        vp = rects[_VIEWPORT]
        # Cursor near viewport's left edge → substrate classifies Left →
        # docks console to viewport's left, splitting horizontally.
        outcome = _drop(tf, _CONSOLE, _left_edge(vp), _panels_payload(rects))
        assert isinstance(outcome, str) and "console" in outcome, (
            f"E.1 split-insert returns an outcome ({outcome})"
        )
        topo = _topology(tf)
        after = _panel_ids(topo)
        assert len(after) == 5, f"E.2 leaf count held at 5 (a move, not a spawn): {after}"
        assert _CONSOLE in after, "E.3 console still present"
        # Left drop → console takes the first slot beside viewport, so it
        # immediately precedes viewport in depth-first order.
        ci, vi = after.index(_CONSOLE), after.index(_VIEWPORT)
        assert ci + 1 == vi, f"E.4 console docked immediately left of viewport ({after})"
        new_splits = [s for s in _split_ids(topo) if s.startswith(_REORG_SPLIT_PREFIX)]
        assert new_splits == [f"{_REORG_SPLIT_PREFIX}0"], (
            f"E.5 exactly one reorg split minted, got {new_splits}"
        )
        assert _split_seq(tf) == 1, "E.6 split_seq bumped to 1"

        # ─── (E2) the reorg-minted splitter is interactive ───────────
        _section("E2: runtime-minted splitter is drag-resizable (R688)")
        reorg_split = f"{_REORG_SPLIT_PREFIX}0"
        # Pre-R688 this divider had NO SplitterExternal (the external set
        # was frozen at boot), so it rendered but was inert + unqueryable.
        # R688's reconcile_externals registers it on the topology change.
        ratio_before = _splitter_ratio(tf, reorg_split)
        assert ratio_before is not None, (
            "E2.1 the runtime split now resolves an External (runtime registration)"
        )
        assert tf.query(f"/{reorg_split}/external/orientation") == "horizontal", (
            "E2.2 left-edge drop produced a horizontal divider"
        )
        # Drag the divider's handle (children[1] of the splitter Container)
        # to a new x; the SplitterExternal's cursor-fraction drag must
        # move the ratio Signal — proving the new surface is truly live,
        # not just resolvable.
        splitter = _find_node(_layout(tf), reorg_split)
        assert splitter is not None, "E2.3 splitter node present in layout"
        children = splitter.get("children") or []
        assert len(children) >= 3, "E2.4 splitter is [left, handle, right]"
        handle = children[1].get("rect") or {}
        hy = float(handle.get("y", 0)) + float(handle.get("h", 1)) * 0.5
        sp_rect = splitter.get("rect") or {}
        # Drag from the handle toward the splitter's 30%-width mark.
        from_x = float(handle.get("x", 0)) + float(handle.get("w", 4)) * 0.5
        to_x = float(sp_rect.get("x", 0)) + float(sp_rect.get("w", 0)) * 0.3
        drag_resp = tf.request(
            "scene/drag",
            {"from": {"x": from_x, "y": hy}, "to": {"x": to_x, "y": hy}, "steps": 4},
        )
        assert drag_resp is not None, "E2.5 scene/drag on the runtime splitter returns"
        wait_until(
            lambda: _splitter_ratio(tf, reorg_split) is not None,
            desc="E2.6 ratio still readable post-drag",
        )
        ratio_after = _splitter_ratio(tf, reorg_split)
        assert 0.05 <= ratio_after <= 0.95, (
            f"E2.7 post-drag ratio {ratio_after} inside the splitter clamp"
        )
        assert abs(ratio_after - ratio_before) > 1e-3, (
            f"E2.8 dragging the runtime splitter moved its ratio "
            f"({ratio_before} -> {ratio_after}) — the new surface is live"
        )

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

        # ─── (G) symbolic path + rejected gestures ───────────────────
        _section("G: symbolic reorganize + rejected gestures")
        # Symbolic path (AI-agent style, no pixels): swap two panels by
        # id + zone name. Proves the second wire form is live.
        sym_before = _panel_ids(_topology(tf))
        outcome = _reorganize(tf, _PROPERTIES, _OUTLINER, "Center")
        assert isinstance(outcome, str), f"G.1 symbolic reorganize returns outcome ({outcome!r})"
        sym_after = _panel_ids(_topology(tf))
        assert sym_after != sym_before, "G.2 symbolic swap changed the topology"
        assert sym_after.index(_PROPERTIES) == sym_before.index(_OUTLINER), (
            "G.3 properties took outliner's slot"
        )
        before = _topology(tf)
        # Stale source via the geometry path: cursor over a real panel but
        # source names no leaf → resolve builds a gesture, apply rejects.
        try:
            _drop(tf, "ghost", _center(rects[_VIEWPORT]), _panels_payload(rects))
            raise AssertionError("G.4 stale source must reject")
        except RpcError as exc:
            assert exc.code != 0, f"G.4 stale source rejected (code {exc.code})"
        # Unknown zone via the symbolic path → reject.
        try:
            _reorganize(tf, _VIEWPORT, _PROPERTIES, "Diagonal")
            raise AssertionError("G.5 unknown zone must reject")
        except RpcError as exc:
            assert exc.code != 0, f"G.5 unknown zone rejected (code {exc.code})"
        # Malformed drop payload (missing cursor) → reject.
        try:
            tf.invoke(
                f"/{_REORG_TAG}/external/drop",
                {"source": _VIEWPORT, "panels": _panels_payload(rects)},
            )
            raise AssertionError("G.6 malformed drop must reject")
        except RpcError as exc:
            assert exc.code != 0, f"G.6 malformed drop rejected (code {exc.code})"
        assert _topology(tf) == before, "G.7 rejected gestures leave topology unchanged"

        # ─── (H) determinism ─────────────────────────────────────────
        _section("H: query determinism")
        snap_a = _topology(tf)
        snap_b = _topology(tf)
        assert snap_a == snap_b, "H.1 back-to-back topology queries are identical"
        assert _panel_ids(snap_a) == _panel_ids(snap_b), "H.2 panel order stable"

        # ─── (I) R749 §5.52 reorganize undo/redo ─────────────────────
        # `DockReorganizeExternal::with_undo` records each gesture as a
        # reversible SignalEdit<DockTopology> (the 3rd UndoCommand consumer
        # — editor workspace history); the UndoStackExternal surfaces it.
        _section("I: reorganize undo/redo")
        undo = "/dock_undo_stack/external"
        before_i = _panel_ids(_topology(tf))
        # A symbolic swap is recorded onto the shared history.
        _reorganize(tf, _OUTLINER, _CONSOLE, "Center")
        after_i = _panel_ids(_topology(tf))
        assert after_i != before_i, "I.1 reorganize changed the layout"
        assert int(tf.query(f"{undo}/count")) >= 1, "I.2 reorganize recorded onto the undo stack"
        assert tf.query(f"{undo}/can_undo") is True, "I.3 history is undoable"
        assert tf.query(f"{undo}/undo_label") is not None, "I.4 the edit carries a label"
        # Undo restores the prior layout; redo re-applies it.
        assert tf.invoke(f"{undo}/undo", None) is True, "I.5 undo stepped the cursor back"
        assert _panel_ids(_topology(tf)) == before_i, "I.6 undo restored the layout"
        assert tf.query(f"{undo}/can_redo") is True, "I.7 the undone edit is redoable"
        assert tf.invoke(f"{undo}/redo", None) is True, "I.8 redo stepped the cursor forward"
        assert _panel_ids(_topology(tf)) == after_i, "I.9 redo re-applied the layout"

        print("[demo] r686_dock_reorganize: all sections PASS")


if __name__ == "__main__":
    sys.exit(run_demo("r686_dock_reorganize", body))
