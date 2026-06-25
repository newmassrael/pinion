#!/usr/bin/env python3
"""R1097 §5.51 PR-32 — live-pointer dock reorganize / tabify (the unified
threshold-detach model end-to-end).

PR-32 made tear-off detach **threshold-based** (a header drag past
DRAG_CLICK_THRESHOLD_PX tears the panel into a follower, even inside a dock
zone) rather than escape-based. The follower (visual) and the drop decision
are decoupled: the gesture stays captured by the SOURCE window's router, so a
drop **on a dock zone** still resolves through `resolve_drop_point` and
redocks/reorganizes (criterion 2), while a drop in empty space floats. This
demo drives the criterion-2 path with a **real `scene/drag`** on a panel
header — begin_drag → drag_to_at (threshold detach + zone preview) →
drag_release over the target zone → `DockReorganizer::apply_intent` — and
observes the reorganized topology, the §2 #2 RPC-as-primary-path contract.

The threshold-detach *timing* itself is a during-drag transition the atomic
`scene/drag` resets on release (a within-dock drop redocks, clearing
`detached`), so it is pinned at the widget layer by the `r1097_*` unit tests;
this demo pins the end-to-end pointer→reorganize / pointer→tabify outcomes
(distinct from r686, which drives the same edits through the `invoke` wire).

Section roadmap (>=30 assertions across A-F):

  (A) Boot sanity — 5 panels canonical depth-first, 4 boot splits, no tab
      wells, split_seq / tabs_seq both 0.
  (B) Pointer EDGE drag = split-insert — `scene/drag` a panel header onto a
      sibling's left-edge zone docks it there (a `reorg-split-N` divider
      appears, panel count holds at 5, split_seq bumps).
  (C) Pointer CENTER drag = tabify — `scene/drag` a header onto a sibling's
      centre stacks them into a `Tabs` well (tabs_seq bumps, panel count
      holds).
  (D) Un-dragged panels intact — the panels never dragged keep their ids.
  (E) Reorganize is a move — no panel created or destroyed across the gestures.
  (F) Determinism — back-to-back topology reads agree.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo, wait_until  # noqa: E402

_MAIN_W = 1200
_MAIN_H = 800

_TOOLBAR = "toolbar"
_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_CANONICAL = [_TOOLBAR, _OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE]

_REORG_TAG = "dock_reorganize"
_REORG_SPLIT_PREFIX = "reorg-split-"


# ─── topology JSON walkers (Tabs-aware, mirror the Rust collectors) ──


def _panel_ids(topo: Any) -> list[str]:
    out: list[str] = []

    def walk(node: Any) -> None:
        if not isinstance(node, dict):
            return
        kind = node.get("type")
        if kind == "Leaf":
            out.append(node["panel_id"])
        elif kind == "Tabs":
            out.extend(node["panels"])
        elif kind == "Split":
            walk(node["first"])
            walk(node["second"])

    walk(topo.get("root"))
    return out


def _split_ids(topo: Any) -> list[str]:
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


def _tabs_wells(topo: Any) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []

    def walk(node: Any) -> None:
        if not isinstance(node, dict):
            return
        kind = node.get("type")
        if kind == "Tabs":
            out.append(node)
        elif kind == "Split":
            walk(node["first"])
            walk(node["second"])

    walk(topo.get("root"))
    return out


def _topology(tf: RpcSubprocess) -> Any:
    return tf.query(f"/{_REORG_TAG}/external/topology")


def _split_seq(tf: RpcSubprocess) -> int:
    return int(tf.query(f"/{_REORG_TAG}/external/split_seq"))


def _tabs_seq(tf: RpcSubprocess) -> int:
    return int(tf.query(f"/{_REORG_TAG}/external/tabs_seq"))


# ─── layout + drag drivers ──────────────────────────────────────────


def _layout(tf: RpcSubprocess) -> Any:
    resp = tf.request("scene/layout", {"viewport": {"width": _MAIN_W, "height": _MAIN_H}})
    assert resp is not None
    return resp.result


def _find_rect(layout: Any, tag: str) -> Optional[dict[str, float]]:
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


def _rect(tf: RpcSubprocess, tag: str) -> dict[str, float]:
    rect = _find_rect(_layout(tf), tag)
    assert rect is not None, f"{tag} rect must be present in scene/layout"
    return rect


def _left_edge(rect: dict[str, float]) -> tuple[float, float]:
    return rect["x"] + rect["w"] * 0.05, rect["y"] + rect["h"] * 0.5


def _center(rect: dict[str, float]) -> tuple[float, float]:
    return rect["x"] + rect["w"] * 0.5, rect["y"] + rect["h"] * 0.5


def _drag_header(tf: RpcSubprocess, panel: str, to_at: tuple[float, float], steps: int = 8) -> None:
    """Live-pointer drag: press a panel's `{panel}#header`, drag past the
    detach threshold, release at `to_at` (window-local logical px). The
    interpolated steps cross DRAG_CLICK_THRESHOLD_PX so the gesture detaches
    (PR-32) before the drop resolves the target zone."""
    tf.request(
        "scene/drag",
        {
            "window": "main",
            "from_path": f"{panel}#header",
            "to": {"x": float(to_at[0]), "y": float(to_at[1])},
            "steps": int(steps),
        },
    )


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot sanity ──────────────────────────────────────────
        _section("A: boot topology sanity")
        topo = _topology(tf)
        assert _panel_ids(topo) == _CANONICAL, f"A.1 canonical panel order, got {_panel_ids(topo)}"
        assert len(_split_ids(topo)) == 4, "A.2 four boot splits"
        assert _tabs_wells(topo) == [], "A.3 no tab wells at boot"
        assert _split_seq(tf) == 0, "A.4 split_seq starts at 0"
        assert _tabs_seq(tf) == 0, "A.5 tabs_seq starts at 0"

        # ── (B) pointer EDGE drag = split-insert (criterion 2) ───────
        _section("B: pointer drag onto a left-edge zone reorganizes")
        vp = _rect(tf, _VIEWPORT)
        # Drag the console header onto the viewport's left edge → the
        # substrate classifies Left → docks console beside the viewport
        # (a move through the live pointer coordinator, not the invoke wire).
        _drag_header(tf, _CONSOLE, _left_edge(vp))
        wait_until(
            lambda: any(s.startswith(_REORG_SPLIT_PREFIX) for s in _split_ids(_topology(tf))),
            desc="B.1 a reorg-split divider appears after the pointer dock",
        )
        topo = _topology(tf)
        after = _panel_ids(topo)
        assert len(after) == 5, f"B.2 panel count held at 5 (a move): {after}"
        assert _CONSOLE in after and _VIEWPORT in after, "B.3 both panels still present"
        ci, vi = after.index(_CONSOLE), after.index(_VIEWPORT)
        assert ci + 1 == vi, f"B.4 console docked immediately left of viewport ({after})"
        new_splits = [s for s in _split_ids(topo) if s.startswith(_REORG_SPLIT_PREFIX)]
        assert new_splits == [f"{_REORG_SPLIT_PREFIX}0"], f"B.5 exactly one reorg split, {new_splits}"
        assert _split_seq(tf) == 1, "B.6 split_seq bumped to 1"
        assert _tabs_seq(tf) == 0, "B.7 an edge drop mints no tab well"
        # Layout reflects the new docking: console now sits left of viewport.
        console_rect = _rect(tf, _CONSOLE)
        viewport_rect = _rect(tf, _VIEWPORT)
        assert console_rect["x"] < viewport_rect["x"], (
            f"B.8 console docked left of viewport (c.x={console_rect['x']}, v.x={viewport_rect['x']})"
        )
        assert console_rect["w"] > 0 and console_rect["h"] > 0, "B.9 relocated console non-degenerate"

        # ── (C) pointer CENTER drag = tabify ─────────────────────────
        _section("C: pointer drag onto a centre zone tabifies")
        props = _rect(tf, _PROPERTIES)
        wells_before = len(_tabs_wells(_topology(tf)))
        # Drag the outliner header onto the properties centre → Tabify.
        _drag_header(tf, _OUTLINER, _center(props))
        wait_until(
            lambda: len(_tabs_wells(_topology(tf))) == wells_before + 1,
            desc="C.1 a tab well appears after the pointer centre drop",
        )
        topo = _topology(tf)
        wells = _tabs_wells(topo)
        assert len(wells) == 1, f"C.2 exactly one tab well, got {len(wells)}"
        well = wells[0]
        assert well["id"].startswith("reorg-tabs"), f"C.3 minted well id ({well['id']!r})"
        assert _OUTLINER in well["panels"] and _PROPERTIES in well["panels"], (
            f"C.4 the well stacks the two dropped panels, got {well['panels']}"
        )
        assert _tabs_seq(tf) == 1, "C.5 tabs_seq bumped to 1"
        assert len(well["panels"]) == 2, f"C.6 a two-panel well, got {well['panels']}"
        assert 0 <= well["active"] < len(well["panels"]), f"C.7 active in range ({well['active']})"
        assert _split_seq(tf) == 1, "C.8 a centre drop mints no new split divider"

        # ── (G) a11y mirrors the pointer-built well (ties to R1095) ──
        _section("G: the pointer-built well exposes a11y")
        acc = tf.request("scene/access", {})
        assert acc is not None and acc.result is not None, "G.1 scene/access answers"
        nodes = acc.result.get("nodes")
        assert isinstance(nodes, list), "G.2 access nodes is a list"
        tablists = [n for n in nodes if n.get("role") == "tablist"]
        assert len(tablists) == 1, f"G.3 the pointer tabify produced one tablist, got {len(tablists)}"
        assert tablists[0].get("tag") == well["id"], (
            f"G.4 the tablist tag is the well id ({tablists[0].get('tag')!r} vs {well['id']!r})"
        )
        tabs = [n for n in nodes if n.get("role") == "tab"]
        assert len(tabs) == 2, f"G.5 two tab a11y nodes, got {len(tabs)}"

        # ── (D) un-dragged panels intact ─────────────────────────────
        _section("D: un-dragged panels keep their identity")
        ids = _panel_ids(_topology(tf))
        for panel in (_TOOLBAR, _VIEWPORT):
            assert panel in ids, f"D.{panel} the un-dragged {panel} is still present"

        # ── (E) reorganize is a move (no panel created/destroyed) ────
        _section("E: every gesture is a move")
        assert sorted(_panel_ids(_topology(tf))) == sorted(_CANONICAL), (
            f"E.1 the panel set is conserved across both gestures, got {_panel_ids(_topology(tf))}"
        )

        # ── (F) determinism ──────────────────────────────────────────
        _section("F: determinism")
        a = _topology(tf)
        b = _topology(tf)
        assert a == b, "F.1 back-to-back topology reads are identical"
        assert _panel_ids(a) == _panel_ids(b), "F.2 panel order stable"

        print("[demo] r1097_dock_pointer_reorganize: all sections PASS")


if __name__ == "__main__":
    sys.exit(run_demo("r1097_dock_pointer_reorganize", body))
