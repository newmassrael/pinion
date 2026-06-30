#!/usr/bin/env python3
"""R1158 §5.51 — drag a tab OUT of its well to FLOAT it (position-dependent).

SCOPE — read this before the assertions. A live user merged two panels into a
TAB well and then dragged a tab fully OUT of the window expecting it to FLOAT
(VS Code style). The R1156 v1 drag-to-undock IGNORED the drop position: ANY drag
of a tab called `undock_tab` (split it beside its well sibling), so dragging a
tab out split it to the side instead of floating. R1158 makes a tab drag a
PANEL-header drag — it branches on WHERE it lands:

  * dragged OUT of every dock zone  → FLOAT (a `torn-<panel>` window; the tab
    leaves the well via `float_out_panel` — a tab is always collapse-style, a
    placeholder TAB makes no sense),
  * over another panel's dock zone  → DOCK there (`dock_panel_at_zone`),
  * over another window's dock zone  → cross-window redock.

The proof is AI-observable scene-as-data: `scene/windows` gains a `torn-<panel>`
floating window on a drag-OUT (and NONE on a drag-to-dock), and the topology JSON
loses / relocates the dragged tab accordingly.

Section roadmap (>=30 assertions across A-E):

  (A) Boot — 5 panels, NO tab well, NO floating window.
  (B) Tabify viewport+properties (centre reorganize) — one Tabs well [properties,
      viewport]; find its dynamic well id from the painted tablist.
  (C) HEADLINE — drag tab 0 (properties) fully OUT of the window: a
      `torn-properties` floating window appears and the well collapses (properties
      left the dock). This is the bug the user hit: a drag-out now FLOATS.
  (D) CONTRAST — tabify two still-docked panels, then drag a tab over ANOTHER
      panel's zone: it DOCKS there (relocates in the dock) and NO new floating
      window appears. Position decides float-vs-dock.
  (E) Determinism — back-to-back window/topology reads agree.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, assert_eq, run_demo, wait_until  # noqa: E402

EXAMPLE = "hello-dock-panels-editor"
REORG = "/dock_reorganize/external"
MAIN = "main"
MW, MH = 1200, 800
PANELS = {"toolbar", "outliner", "viewport", "properties", "console"}


def _topology(tf: RpcSubprocess) -> Any:
    return tf.query(f"{REORG}/topology")


def _wells(node: Any) -> int:
    """Count `Tabs` wells (a node carrying a `panels` list) in the topology."""
    n = 0
    if isinstance(node, dict):
        if "panels" in node:
            n += 1
        for v in node.values():
            n += _wells(v)
    elif isinstance(node, list):
        for x in node:
            n += _wells(x)
    return n


def _docked_panels(node: Any, out: set | None = None) -> set:
    out = set() if out is None else out
    if isinstance(node, dict):
        if node.get("type") == "Leaf" and node.get("panel_id"):
            out.add(node["panel_id"])
        for p in node.get("panels", []):
            out.add(p)
        for v in node.values():
            _docked_panels(v, out)
    elif isinstance(node, list):
        for x in node:
            _docked_panels(x, out)
    return out


def _well_panels(node: Any) -> list[list[str]]:
    """Every Tabs well's panel list."""
    found: list[list[str]] = []
    if isinstance(node, dict):
        if "panels" in node:
            found.append(list(node["panels"]))
        for v in node.values():
            found.extend(_well_panels(v))
    elif isinstance(node, list):
        for x in node:
            found.extend(_well_panels(x))
    return found


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    return resp.result.get("windows") or []


def _window_ids(tf: RpcSubprocess) -> set:
    return {w.get("id") for w in _windows(tf)}


def _torn_windows(tf: RpcSubprocess) -> set:
    return {wid for wid in _window_ids(tf) if isinstance(wid, str) and wid.startswith("torn-")}


def _tablist(tf: RpcSubprocess) -> Optional[dict]:
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    nodes = resp.result.get("nodes") or []
    return next((n for n in nodes if n.get("role") == "tablist"), None)


def _well_id(tf: RpcSubprocess) -> Optional[str]:
    tl = _tablist(tf)
    return tl["tag"] if tl else None


def _reorganize(tf: RpcSubprocess, source: str, target: str, zone: str) -> Any:
    return tf.invoke(f"{REORG}/reorganize", {"source": source, "target": target, "zone": zone})


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=2.0) as tf:
        # ── (A) boot ────────────────────────────────────────────────
        _section("A: boot — no tab well, no floating window")
        topo = _topology(tf)
        assert_eq(_wells(topo), 0, "A.1 boot has no tab well")
        assert_eq(_docked_panels(topo), PANELS, "A.2 all 5 panels docked")
        assert_eq(_torn_windows(tf), set(), "A.3 no floating window at boot")
        assert_eq(_window_ids(tf), {MAIN}, "A.4 only the main window declared")

        # ── (B) tabify viewport + properties ────────────────────────
        _section("B: tabify viewport+properties → a Tabs well")
        out = _reorganize(tf, "viewport", "properties", "Center")
        assert out is not None, "B.1 tabify invoke answered"
        wait_until(lambda: _wells(_topology(tf)) == 1, desc="B.2 a tab well appears")
        well_id = wait_until(lambda: _well_id(tf), desc="B.3 the tablist paints")
        assert isinstance(well_id, str) and well_id.startswith("reorg-tabs"), (
            f"B.4 minted well id ({well_id!r})"
        )
        wells = _well_panels(_topology(tf))
        assert_eq(len(wells), 1, "B.5 exactly one well")
        # reorganize(source, target, Center) stacks [target, source].
        assert_eq(wells[0], ["properties", "viewport"], "B.6 the well stacks [properties, viewport]")
        assert_eq(_docked_panels(_topology(tf)), PANELS, "B.7 no panel lost in the tabify")
        assert_eq(_torn_windows(tf), set(), "B.8 tabify created no floating window")
        # The TabWellExternal is runtime-registered at the (dynamic) well id, so
        # the drag source actually resolves (the whole gesture chain is wired).
        assert_eq(tf.query(f"/{well_id}/external/well_id"), well_id, "B.9 the well external is live")
        assert_eq(len(wells[0]), 2, "B.10 the well holds two tabs")

        # ── (C) HEADLINE: drag a tab OUT → it FLOATS ─────────────────
        _section("C: drag tab 0 (properties) OUT of the window → FLOAT")
        # Press on the painted `{well_id}#0` tab and march fully OUT of the
        # 1200x800 window (release at 2000,400 = past the right edge): the
        # router resolves `over = None` (escaped every dock zone), so the tab
        # FLOATS instead of splitting beside its sibling (the R1156 v1 bug).
        tf.drag(from_path=f"{well_id}#0", to_at=(2000.0, 400.0))
        wait_until(
            lambda: "torn-properties" in _torn_windows(tf),
            desc="C.1 a torn-properties floating window appears",
        )
        assert "torn-properties" in _window_ids(tf), "C.2 properties floated into its own window"
        wait_until(lambda: _wells(_topology(tf)) == 0, desc="C.3 the well collapsed")
        t_c = _topology(tf)
        assert "properties" not in _docked_panels(t_c), "C.4 properties left the dock (it floated)"
        assert {"toolbar", "outliner", "viewport", "console"} <= _docked_panels(t_c), (
            "C.5 the other four panels stay docked"
        )
        assert_eq(_wells(t_c), 0, "C.6 no tab well remains (the 2-tab well collapsed to its sibling)")
        assert_eq(_window_ids(tf), {MAIN, "torn-properties"}, "C.7 exactly main + the new floater")
        assert _tablist(tf) is None, "C.8 no tablist paints once the well collapsed"
        assert "viewport" in _docked_panels(t_c), "C.9 the well's other tab stayed docked (a leaf)"

        # ── (D) CONTRAST: drag a tab over a zone → it DOCKS ──────────
        _section("D: drag a tab over another panel's zone → DOCK (no new floater)")
        # Tabify two still-docked panels into a fresh well.
        _reorganize(tf, "console", "outliner", "Center")
        wait_until(lambda: _wells(_topology(tf)) == 1, desc="D.1 a fresh well appears")
        well2 = wait_until(lambda: _well_id(tf), desc="D.2 the fresh tablist paints")
        torn_before = _torn_windows(tf)
        # Drag tab 0 of the fresh well onto the viewport panel's CENTRE: it docks
        # WITH viewport (a relocation in the dock), NOT a float — no new window.
        wells_before = _well_panels(_topology(tf))
        moved_tab = wells_before[0][0]  # the panel at tab 0 of the fresh well
        tf.drag(from_path=f"{well2}#0", to_path="viewport")
        wait_until(
            lambda: moved_tab in _well_panels_with("viewport", tf),
            desc="D.3 the dragged tab docked WITH viewport",
        )
        assert_eq(_torn_windows(tf), torn_before, "D.4 docking a tab created NO new floating window")
        assert moved_tab in _docked_panels(_topology(tf)), "D.5 the tab is still docked (it relocated)"
        # The headline floater is still floating (untouched by the dock).
        assert "torn-properties" in _window_ids(tf), "D.6 the earlier floater is unaffected"
        # The dragged tab now shares a well WITH viewport (it docked at the centre).
        assert "viewport" in _well_panels_with(moved_tab, tf), "D.7 the tab tabified with viewport"
        assert_eq(len(_torn_windows(tf)), 1, "D.8 still exactly one floating window total")

        # ── (E) determinism ─────────────────────────────────────────
        _section("E: determinism")
        assert_eq(_window_ids(tf), _window_ids(tf), "E.1 window set reads are stable")
        assert_eq(_wells(_topology(tf)), _wells(_topology(tf)), "E.2 well count reads are stable")
        assert_eq(_docked_panels(_topology(tf)), _docked_panels(_topology(tf)), "E.3 docked set stable")
        assert_eq(_torn_windows(tf), _torn_windows(tf), "E.4 floating set stable")

        print("[demo] r1158_tab_drag_float: all sections PASS (drag-out floats, drag-over docks)")


def _well_panels_with(panel: str, tf: RpcSubprocess) -> list[str]:
    """Flatten of every well that contains `panel` (D.3 helper)."""
    out: list[str] = []
    for w in _well_panels(_topology(tf)):
        if panel in w:
            out.extend(w)
    return out


if __name__ == "__main__":
    sys.exit(run_demo("r1158_tab_drag_float", body))
