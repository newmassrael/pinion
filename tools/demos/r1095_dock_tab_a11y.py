#!/usr/bin/env python3
"""R1095 §5.51 §5.27 §5.40 §2 #7 — dock tab-well accessibility.

R1083 gave the dock `DockNode::Tabs` wells (tabbed docking) and R1085 made them
AI-activatable (`activate_tab`), but they emitted NO accessibility tree — a
screen reader could not announce a tab well or which tab was selected. R1095
closes that: the dock crate's `dock_tablist_access_nodes` walks the live
topology and emits WAI-ARIA `tablist` / `tab` / `tabpanel` AccessNodes (the 3rd
consumer of the lifted `pinion_a11y::tablist_tab_nodes`, after hello-tabs /
hello-tab-reorder), and `hello-dock-panels-editor` contributes them from its
`WidgetA11y::access_node`. Tags mirror the painted scene — the tablist is the
well id, each tab is `{well_id}#{i}`, the tabpanel is the active panel — so an
AI/AT reads the same structure the user sees, observable over `scene/access`.

The tab wells appear after a `reorganize`-Center (Tabify); the demo creates one,
then asserts its a11y. The pointer-click-to-switch (b) gesture is the next
slice; AI/RPC drives selection here via `activate_tab` (R1085).

Section roadmap (>=30 assertions across A-F):

  (A) Boot — the editor declares no tab well, so `scene/access` carries no
      `tablist` (a docked panel is not a tab).
  (B) Tabify creates a tab well + its a11y — `reorganize` Center tabifies two
      panels; `scene/access` grows exactly one `tablist` whose tag is the well
      id, owning two `tab` children with aria-posinset/setsize, exactly one
      aria-selected, plus a `tabpanel`.
  (C) `activate_tab` moves aria-selected — switching the active tab over RPC
      moves `selected` to the addressed tab and repoints the `tabpanel`.
  (D) Tags mirror the paint — every a11y tab tag `{well_id}#{i}` is present in
      the painted scene (paint <-> a11y tag parity, the enrichment contract).
  (E) Roving descendant — focusing the well's strip marks its active tab
      `state.focused` (the WAI-ARIA roving active descendant); inactive tabs
      are not.
  (F) Structural invariants — exactly one tablist throughout; re-activating
      the already-active tab is an accepted no-op.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

# Editor panel ids (mirrored from the binding).
_OUTLINER = "outliner"
_PROPERTIES = "properties"

_REORG_TAG = "dock_reorganize"


# ─── scene/access helpers ────────────────────────────────────────────


def _access(tf: RpcSubprocess) -> dict:
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    result = resp.result
    assert isinstance(result.get("nodes"), list), f"scene/access.nodes must be a list; got {result!r}"
    return result


def _nodes(tf: RpcSubprocess) -> list[dict]:
    return _access(tf)["nodes"]


def _by_role(nodes: list[dict], role: str) -> list[dict]:
    return [n for n in nodes if n.get("role") == role]


def _by_tag(nodes: list[dict], tag: str) -> Optional[dict]:
    for n in nodes:
        if n.get("tag") == tag:
            return n
    return None


def _tab_tag(well_id: str, index: int) -> str:
    return f"{well_id}#{index}"


# ─── reorganize / activate drivers ───────────────────────────────────


def _reorganize(tf: RpcSubprocess, source: str, target: str, zone: str) -> Any:
    return tf.invoke(
        f"/{_REORG_TAG}/external/reorganize",
        {"source": source, "target": target, "zone": zone},
    )


def _activate_tab(tf: RpcSubprocess, well_id: str, index: int) -> Any:
    return tf.invoke(
        f"/{_REORG_TAG}/external/activate_tab",
        {"well_id": well_id, "index": index},
    )


def _wait_tablist(tf: RpcSubprocess) -> dict:
    """Tabify commits reactively; poll scene/access until a tablist appears."""
    return wait_until(
        lambda: next((n for n in _nodes(tf) if n.get("role") == "tablist"), None),
        desc="scene/access grows a dock tablist",
    )


def _selected_index(tf: RpcSubprocess, well_id: str, count: int) -> Optional[int]:
    nodes = _nodes(tf)
    for i in range(count):
        tab = _by_tag(nodes, _tab_tag(well_id, i))
        if tab is not None and tab.get("selected") is True:
            return i
    return None


# ─── paint cross-check ───────────────────────────────────────────────


def _scene_contains_tag(scene: Any, target: str) -> bool:
    if not isinstance(scene, dict):
        return False
    if scene.get("tag") == target:
        return True
    for child in scene.get("children") or []:
        if _scene_contains_tag(child, target):
            return True
    content = scene.get("content")
    return isinstance(content, dict) and _scene_contains_tag(content, target)


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — no tab well, so no tablist a11y ────────────
        boot_nodes = _nodes(tf)
        assert _by_role(boot_nodes, "tablist") == [], (
            "the editor boots Leaf/Split — no tab well, no tablist a11y"
        )

        # ── (B) tabify creates a tab well + its a11y ──────────────
        outcome = _reorganize(tf, _OUTLINER, _PROPERTIES, "Center")
        assert isinstance(outcome, str), f"reorganize Center returns an outcome; got {outcome!r}"
        tablist = _wait_tablist(tf)
        well_id = tablist["tag"]
        assert isinstance(well_id, str) and well_id, f"tablist tag is the well id; got {tablist!r}"
        assert well_id.startswith("reorg-tabs"), (
            f"the minted well id has the reorg-tabs prefix; got {well_id!r}"
        )
        assert tablist.get("role") == "tablist", "the strip node is a tablist"
        nodes = _nodes(tf)
        # Exactly one tablist.
        assert len(_by_role(nodes, "tablist")) == 1, "tabify yields exactly one tablist"
        # The tablist owns two tab children, in order.
        children = tablist.get("children") or []
        assert children == [_tab_tag(well_id, 0), _tab_tag(well_id, 1)], (
            f"tablist owns the per-tab tags in order; got {children!r}"
        )
        # Two tab nodes with aria-posinset / aria-setsize.
        tabs = _by_role(nodes, "tab")
        assert len(tabs) == 2, f"one tab node per panel; got {len(tabs)}"
        assert _tab_tag(well_id, 0) != _tab_tag(well_id, 1), "tab tags are distinct"
        assert {t["tag"] for t in tabs} == {_tab_tag(well_id, 0), _tab_tag(well_id, 1)}, (
            f"tab node tags are the well's composite tags; got {[t['tag'] for t in tabs]!r}"
        )
        for i, expect_pos in ((0, 1), (1, 2)):
            tab = _by_tag(nodes, _tab_tag(well_id, i))
            assert tab is not None, f"tab {i} present in scene/access"
            assert tab.get("size_of_set") == 2, f"tab {i} aria-setsize is 2; got {tab!r}"
            assert tab.get("position_in_set") == expect_pos, (
                f"tab {i} aria-posinset is {expect_pos} (1-based); got {tab!r}"
            )
        # Exactly one tab is aria-selected.
        selected = [i for i in (0, 1) if (_by_tag(nodes, _tab_tag(well_id, i)) or {}).get("selected")]
        assert len(selected) == 1, f"exactly one tab is aria-selected; got {selected!r}"
        active0 = selected[0]
        # A tabpanel exists, naming the active panel (one of the tabified ids).
        panels = _by_role(nodes, "tabpanel")
        assert len(panels) == 1, f"one tabpanel for the well; got {len(panels)}"
        assert panels[0].get("tag") in (_OUTLINER, _PROPERTIES), (
            f"tabpanel tag is the active tabified panel id; got {panels[0]!r}"
        )

        # ── (C) activate_tab moves aria-selected ──────────────────
        other = 1 - active0
        _activate_tab(tf, well_id, other)
        wait_until(
            lambda: _selected_index(tf, well_id, 2) == other,
            desc=f"activate_tab moves aria-selected to {other}",
        )
        nodes = _nodes(tf)
        assert (_by_tag(nodes, _tab_tag(well_id, other)) or {}).get("selected") is True, (
            "the activated tab is aria-selected"
        )
        assert (_by_tag(nodes, _tab_tag(well_id, active0)) or {}).get("selected") is False, (
            "the previously active tab is no longer aria-selected"
        )
        # The tabpanel repoints to the newly active panel (a non-empty id).
        panel_after = _by_role(nodes, "tabpanel")[0]
        assert isinstance(panel_after.get("tag"), str) and panel_after["tag"], (
            f"tabpanel tag is the active panel id; got {panel_after!r}"
        )
        assert panel_after["tag"] in (_OUTLINER, _PROPERTIES), (
            f"the repointed tabpanel is one of the tabified panels; got {panel_after!r}"
        )

        # ── (D) a11y tags mirror the painted scene ────────────────
        snap = tf.snapshot(source="paint")
        for i in (0, 1):
            assert _scene_contains_tag(snap, _tab_tag(well_id, i)), (
                f"painted scene must carry the a11y tab tag {_tab_tag(well_id, i)!r} "
                "(paint <-> a11y parity)"
            )
        assert _scene_contains_tag(snap, well_id), "painted scene carries the tablist strip tag"

        # ── (E) roving active descendant ──────────────────────────
        tf.request("focus/set", {"tag": well_id})
        wait_until(
            lambda: ((_by_tag(_nodes(tf), _tab_tag(well_id, other)) or {}).get("state") or {}).get(
                "focused"
            )
            is True,
            desc="focusing the strip marks its active tab focused",
        )
        nodes = _nodes(tf)
        active_state = (_by_tag(nodes, _tab_tag(well_id, other)) or {}).get("state") or {}
        assert active_state.get("focused") is True, "the active tab is the roving descendant"
        inactive_state = (_by_tag(nodes, _tab_tag(well_id, active0)) or {}).get("state") or {}
        assert inactive_state.get("focused") is not True, "an inactive tab is not the roving descendant"

        # ── (F) structural invariants ─────────────────────────────
        # Re-activating the already-active tab is an accepted no-op.
        _activate_tab(tf, well_id, other)
        nodes = _nodes(tf)
        assert len(_by_role(nodes, "tablist")) == 1, "still exactly one tablist"
        assert _selected_index(tf, well_id, 2) == other, (
            "re-activating the active tab keeps it selected (no-op)"
        )
        assert len(_by_role(nodes, "tab")) == 2, "still two tab nodes"
        assert len(_by_role(nodes, "tabpanel")) == 1, "still one tabpanel"


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1095 §5.51 §5.27 §5.40 §2 #7 — dock tab-well accessibility (tablist/tab/tabpanel)",
        body,
    ))
