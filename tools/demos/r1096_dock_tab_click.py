#!/usr/bin/env python3
"""R1096 §5.51 — dock tab-well pointer click-to-switch demo.

Slice (b) of the R1083 tabbed-docking line: R1085 made `DockNode::Tabs`
wells switchable by AI / RPC (`activate_tab` invoke); R1095 gave them an
a11y tree; **R1096 makes them switchable by a real pointer click.**

The dock walker paints each tab well's strip via `view_tabs(well_id, …)`,
tagging each tab `{well_id}#{i}`. The `InputRouter`'s R51.42 `#`-split
protocol resolves a hit on `{well_id}#{i}` to the `External` registered at
the *primary* tag `{well_id}` and dispatches `invoke("send", "{i}:Event")`.
R1096 adds a `TabWellExternal` registered at the (dynamic, `reorg-tabs-{seq}`)
well id — runtime-registered by the editor's `create_extra_externals` re-run
on every topology change (the same R688 reconcile path the per-split /
per-panel externals ride) — that translates the click-release edge into
`DockReorganizer::activate_tab(well_id, i)`: the SAME sole-writer commit
funnel the AI `activate_tab` invoke and the R742 pointer drags pass through.

This demo drives a *real* `scene/click` on the painted tab tag (the full
router → hit-test → `#`-split → `TabWellExternal` → coordinator → topology
chain), not the symbolic `activate_tab` invoke (that is r1095's job).

Section roadmap (>=30 assertions across A-G):

  (A) Boot — the editor is all Leaf/Split, so no tab well + no well
      external resolves.
  (B) Tabify mints a well — a `reorganize` Center stacks two panels into a
      `Tabs` well; the `TabWellExternal` is runtime-registered at the well
      id (the R688 reconcile path), exposing `well_id` + a live `active`
      read sourced from the topology (never stored on the external).
  (C) The headline — a real `scene/click` on the inactive tab's painted
      `{well_id}#{i}` tag flips the active tab through the whole router
      chain; the topology, the well external's `active`, and the a11y
      `aria-selected` all agree.
  (D) Already-active click is a no-op — clicking the visible tab records no
      undo edit (the gesture-layer guard), while a click on a different tab
      commits exactly one reversible edit.
  (E) a11y parity — `aria-selected` tracks the clicked tab; the painted tab
      tags mirror the a11y tab tags.
  (F) The send wire + rejections — the `TabWellExternal`'s `send` channel
      activates on `PointerUp`, no-ops on press / hover edges, rejects an
      out-of-range index, and refuses interventions on its derived reads.
  (G) Determinism — back-to-back reads agree.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

# Editor panel ids + tags (mirrored from the binding).
_OUTLINER = "outliner"
_PROPERTIES = "properties"
_REORG_TAG = "dock_reorganize"
_UNDO = "/dock_undo_stack/external"


# ─── scene/access helpers ────────────────────────────────────────────


def _nodes(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    nodes = resp.result.get("nodes")
    assert isinstance(nodes, list), f"scene/access.nodes must be a list; got {resp.result!r}"
    return nodes


def _by_role(nodes: list[dict], role: str) -> list[dict]:
    return [n for n in nodes if n.get("role") == role]


def _by_tag(nodes: list[dict], tag: str) -> Optional[dict]:
    return next((n for n in nodes if n.get("tag") == tag), None)


def _tab_tag(well_id: str, index: int) -> str:
    return f"{well_id}#{index}"


# ─── topology + well-external drivers ────────────────────────────────


def _reorganize(tf: RpcSubprocess, source: str, target: str, zone: str) -> Any:
    return tf.invoke(
        f"/{_REORG_TAG}/external/reorganize",
        {"source": source, "target": target, "zone": zone},
    )


def _topology(tf: RpcSubprocess) -> Any:
    return tf.query(f"/{_REORG_TAG}/external/topology")


def _wait_tablist(tf: RpcSubprocess) -> dict:
    """Tabify commits reactively; poll scene/access until a tablist appears."""
    return wait_until(
        lambda: next((n for n in _nodes(tf) if n.get("role") == "tablist"), None),
        desc="scene/access grows a dock tablist",
    )


def _well_node(tf: RpcSubprocess, well_id: str) -> Optional[dict]:
    """The `DockNode::Tabs` well node from the reorganize external's topology
    JSON — the SSOT, distinct from the well external's projection of it."""
    found: list[dict] = []

    def walk(node: Any) -> None:
        if not isinstance(node, dict):
            return
        if node.get("type") == "Tabs" and node.get("id") == well_id:
            found.append(node)
        elif node.get("type") == "Split":
            walk(node.get("first"))
            walk(node.get("second"))

    walk(_topology(tf).get("root"))
    return found[0] if found else None


def _topology_active(tf: RpcSubprocess, well_id: str) -> Optional[int]:
    node = _well_node(tf, well_id)
    return node["active"] if node else None


def _well_active(tf: RpcSubprocess, well_id: str) -> Optional[int]:
    """The well external's live `active` projection (R1096 `query`)."""
    val = tf.query(f"/{well_id}/external/active")
    return int(val) if isinstance(val, int) else None


def _a11y_selected(tf: RpcSubprocess, well_id: str, count: int) -> Optional[int]:
    nodes = _nodes(tf)
    for i in range(count):
        tab = _by_tag(nodes, _tab_tag(well_id, i))
        if tab is not None and tab.get("selected") is True:
            return i
    return None


def _undo_count(tf: RpcSubprocess) -> int:
    return int(tf.query(f"{_UNDO}/count"))


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — no tab well, no well external ──────────────────
        _section("A: boot has no tab well")
        assert _by_role(_nodes(tf), "tablist") == [], (
            "A.1 the editor boots Leaf/Split — no tab well, no tablist"
        )
        # No well external resolves at the (not-yet-minted) well id.
        try:
            tf.query("/reorg-tabs-0/external/well_id")
            raise AssertionError("A.2 no well external exists before a tabify")
        except RpcError as exc:
            assert exc.code != 0, f"A.2 well external absent pre-tabify (code {exc.code})"

        # ── (B) tabify mints a well + runtime-registers the external ──
        _section("B: tabify mints a well + its click external")
        outcome = _reorganize(tf, _OUTLINER, _PROPERTIES, "Center")
        assert isinstance(outcome, str), f"B.1 reorganize Center returns an outcome ({outcome!r})"
        tablist = _wait_tablist(tf)
        well_id = tablist["tag"]
        assert well_id.startswith("reorg-tabs"), f"B.2 minted well id ({well_id!r})"
        # R688 reconcile re-ran create_extra_externals → the TabWellExternal
        # is now live at the (dynamic) well id, queryable AT ALL.
        assert tf.query(f"/{well_id}/external/well_id") == well_id, (
            "B.3 the well external is runtime-registered at the well id"
        )
        # The well stacks [target, source] = [properties, outliner], active
        # on the brought-forward source (outliner, index 1).
        well = _well_node(tf, well_id)
        assert well is not None and well["panels"] == [_PROPERTIES, _OUTLINER], (
            f"B.4 the well stacks [target, source], got {well}"
        )
        assert _topology_active(tf, well_id) == 1, "B.5 the dropped source starts active"
        # The external's `active` projection AGREES with the topology SSOT
        # (it reads the topology, never stores its own copy).
        assert _well_active(tf, well_id) == 1, "B.6 well external `active` mirrors the topology"

        # ── (C) headline: a real pointer click flips the active tab ───
        _section("C: pointer click on a tab switches it (full router chain)")
        start = _well_active(tf, well_id)
        assert start is not None
        other = 1 - start
        # A *real* scene/click on the inactive tab's painted tag drives the
        # whole chain: hit-test → R51.42 `#`-split → TabWellExternal →
        # DockReorganizer::activate_tab → topology Signal.
        tf.click(path=_tab_tag(well_id, other))
        wait_until(
            lambda: _well_active(tf, well_id) == other,
            desc="C.1 the click flipped the well external's active tab",
        )
        assert _topology_active(tf, well_id) == other, (
            "C.2 the topology SSOT flipped to the clicked tab"
        )
        assert _a11y_selected(tf, well_id, 2) == other, (
            "C.3 the a11y aria-selected followed the click"
        )
        # Click back to the original tab — the switch is symmetric.
        tf.click(path=_tab_tag(well_id, start))
        wait_until(
            lambda: _well_active(tf, well_id) == start,
            desc="C.4 clicking the other tab switches back",
        )
        assert _topology_active(tf, well_id) == start, "C.5 topology back to the start tab"
        # A tab switch does NOT re-mint the well id (activate_tab is pure
        # nav), so the same external keeps serving across clicks.
        assert tf.query(f"/{well_id}/external/well_id") == well_id, (
            "C.6 the well external persists across switches (no id re-mint)"
        )

        # ── (D) already-active click churns no undo ───────────────────
        _section("D: already-active click is a no-op, a switch commits one edit")
        active_now = _well_active(tf, well_id)
        assert active_now is not None
        undo_before = _undo_count(tf)
        # Clicking the VISIBLE tab must not mint an undo edit (the gesture
        # guard skips activate_tab) — re-click it a few times.
        for _ in range(3):
            tf.click(path=_tab_tag(well_id, active_now))
        assert _undo_count(tf) == undo_before, "D.1 re-clicking the active tab records no undo edit"
        assert _well_active(tf, well_id) == active_now, "D.2 the active tab is unchanged"
        # A click on the OTHER tab does commit exactly one reversible edit.
        switch_to = 1 - active_now
        tf.click(path=_tab_tag(well_id, switch_to))
        wait_until(
            lambda: _well_active(tf, well_id) == switch_to,
            desc="D.3 the real switch took effect",
        )
        assert _undo_count(tf) == undo_before + 1, "D.4 the switch recorded exactly one undo edit"
        assert tf.query(f"{_UNDO}/can_undo") is True, "D.5 the switch is undoable"

        # ── (E) a11y / paint parity ───────────────────────────────────
        _section("E: a11y mirrors paint mirrors topology")
        nodes = _nodes(tf)
        tabs = _by_role(nodes, "tab")
        assert len(tabs) == 2, f"E.1 two tab nodes ({len(tabs)})"
        tab_tags = {t.get("tag") for t in tabs}
        assert tab_tags == {_tab_tag(well_id, 0), _tab_tag(well_id, 1)}, (
            f"E.2 a11y tab tags mirror the painted `{{well_id}}#i` tags ({tab_tags})"
        )
        sel = [i for i in (0, 1) if (_by_tag(nodes, _tab_tag(well_id, i)) or {}).get("selected")]
        assert sel == [_well_active(tf, well_id)], (
            f"E.3 exactly the active tab is aria-selected ({sel})"
        )

        # ── (F) the send wire directly + rejections ───────────────────
        _section("F: the TabWellExternal send wire + rejections")
        send = f"/{well_id}/external/send"
        cur = _well_active(tf, well_id)
        target = 1 - cur
        # The release edge activates (the same wire the router synthesizes).
        out = tf.invoke(send, f"{target}:PointerUp")
        assert isinstance(out, str) and "activate" in out, f"F.1 send PointerUp activates ({out!r})"
        assert _well_active(tf, well_id) == target, "F.2 the send wire switched the tab"
        # A press / hover edge switches nothing.
        assert tf.invoke(send, f"{cur}:PointerDown") is None, "F.3 PointerDown is a no-op"
        assert _well_active(tf, well_id) == target, "F.4 still on the activated tab"
        # A bare release with no `#` sub-index (a press on the strip
        # background between tabs) switches nothing.
        assert tf.invoke(send, "PointerUp") is None, "F.4b bare PointerUp on the strip is a no-op"
        # An out-of-range index is a rejected gesture.
        try:
            tf.invoke(send, "9:PointerUp")
            raise AssertionError("F.5 out-of-range index must reject")
        except RpcError as exc:
            assert exc.code != 0, f"F.5 out-of-range rejected (code {exc.code})"
        assert _well_active(tf, well_id) == target, "F.6 a rejected click leaves active untouched"
        # The derived reads are not interveneable.
        try:
            tf.request("scene/intervene", {"path": f"/{well_id}/external/active", "value": 0})
            raise AssertionError("F.7 active is read-only")
        except RpcError as exc:
            assert exc.code != 0, f"F.7 intervene on active refused (code {exc.code})"

        # ── (G) determinism ───────────────────────────────────────────
        _section("G: determinism")
        a = _topology_active(tf, well_id)
        b = _topology_active(tf, well_id)
        assert a == b == _well_active(tf, well_id), "G.1 topology + external reads agree + are stable"

        print("[demo] r1096_dock_tab_click: all sections PASS")


if __name__ == "__main__":
    sys.exit(run_demo("r1096_dock_tab_click", body))
