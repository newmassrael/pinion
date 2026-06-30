#!/usr/bin/env python3
"""R1159 §5.51 — discrete-target drop resolution: float is reachable INSIDE the window.

The B model (docs/dock-drop-resolution.md) replaces the continuous "every in-panel
point docks, float only outside the window" model with a single `resolve_drop`
SSOT over discrete targets: a panel's EDGE bands split, its CENTRE square tabifies,
and the DEAD-ZONE ring between them (plus any non-panel target like a splitter, and
off every panel) FLOATS. So a tab can be torn off inside a maximized window — the
structural fix for "탭이 창 밖으로 안 빠져" (the continuous model could only float by
leaving the window, impossible when maximized).

Proof is AI-observable scene-as-data: dragging a tab into a panel's dead-zone (or
onto a splitter) makes a `torn-<panel>` window appear; dragging onto a panel's edge
docks it (no floater). Same gesture, position decides — and float no longer needs
the cursor to leave the window.

Section roadmap (>=30 assertions A-E):
  (A) Boot + tabify viewport+properties → a well.
  (B) Drag a tab to another panel's EDGE band → DOCK (split), no floater.
  (C) Drag a tab into a panel's DEAD-ZONE ring → FLOAT (torn window), in-window.
  (D) Drag a tab onto a panel's CENTRE → DOCK (tabify), no new floater.
  (E) Determinism.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, assert_eq, find_by_tag, run_demo, wait_until  # noqa: E402

EXAMPLE = "hello-dock-panels-editor"
REORG = "/dock_reorganize/external"
MW, MH = 1200, 800


def _topology(tf: RpcSubprocess) -> Any:
    return tf.query(f"{REORG}/topology")


def _wells(node: Any) -> int:
    n = 0
    if isinstance(node, dict):
        n += 1 if "panels" in node else 0
        for v in node.values():
            n += _wells(v)
    elif isinstance(node, list):
        for x in node:
            n += _wells(x)
    return n


def _torn(tf: RpcSubprocess) -> set:
    wins = (tf.request("scene/windows", {}).result or {}).get("windows") or []
    return {w.get("id") for w in wins if str(w.get("id", "")).startswith("torn-")}


def _well_id(tf: RpcSubprocess) -> Optional[str]:
    nodes = tf.request("scene/access", {}).result.get("nodes") or []
    tl = next((n for n in nodes if n.get("role") == "tablist"), None)
    return tl["tag"] if tl else None


def _rect(tf: RpcSubprocess, tag: str) -> Optional[dict]:
    snap = tf.snapshot(source="paint", viewport=(MW, MH), window="main")
    node = find_by_tag(snap, tag)
    return node.get("rect") if node else None


def _tabify(tf: RpcSubprocess) -> str:
    tf.invoke(f"{REORG}/reorganize", {"source": "viewport", "target": "properties", "zone": "Center"})
    wait_until(lambda: _wells(_topology(tf)) == 1, desc="a well appears")
    return wait_until(lambda: _well_id(tf), desc="the tablist paints")


def _tab0(tf: RpcSubprocess, well: str) -> str:
    snap = tf.snapshot(source="paint", viewport=(MW, MH), window="main")
    if find_by_tag(snap, f"{well}#0"):
        return f"{well}#0"
    return well


def _drag_tab_to(tf: RpcSubprocess, well: str, x: float, y: float) -> None:
    tf.drag(from_path=_tab0(tf, well), to_at=(x, y), steps=10)
    time.sleep(0.5)


def _section(s: str) -> None:
    print(f"[demo] -- {s}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=2.0) as tf:
        # ── (A) boot + tabify ───────────────────────────────────────
        _section("A: boot + tabify viewport+properties")
        assert_eq(_wells(_topology(tf)), 0, "A.1 no well at boot")
        assert_eq(_torn(tf), set(), "A.2 no floater at boot")
        well = _tabify(tf)
        assert isinstance(well, str) and well.startswith("reorg-tabs"), f"A.3 well id {well!r}"
        assert_eq(_wells(_topology(tf)), 1, "A.4 one well after tabify")
        assert_eq(_torn(tf), set(), "A.5 tabify made no floater")
        con = _rect(tf, "console")
        out_r = _rect(tf, "outliner")
        assert con and out_r, "A.6 console + outliner rects resolve in the paint scene"

        # ── (B) EDGE band → DOCK (split), no floater ────────────────
        _section("B: drag a tab to console's LEFT edge → DOCK (split)")
        # x_rel ~0.08 of console = inside the 0.22 edge band → Left split.
        bx = con["x"] + 0.08 * con["w"]
        by = con["y"] + 0.5 * con["h"]
        torn_before = _torn(tf)
        docked_before = _docked(_topology(tf))
        _drag_tab_to(tf, well, bx, by)
        assert_eq(_torn(tf), torn_before, "B.1 an edge dock created NO floater")
        assert "properties" in _docked(_topology(tf)), "B.2 the tab is still docked (it split in)"
        assert_eq(_wells(_topology(tf)), 0, "B.3 the 2-tab well collapsed (the tab left it)")
        assert_eq(_docked(_topology(tf)), docked_before, "B.4 every panel still docked (relocated, not lost)")
        assert_eq(_torn(tf), set(), "B.5 still zero floaters after an edge dock")

        # reset: re-tabify for the next case
        well = _tabify(tf)
        out_r = _rect(tf, "outliner")
        assert out_r, "B.6 outliner rect resolves"
        assert_eq(_wells(_topology(tf)), 1, "B.7 re-tabified into one well")

        # ── (C) DEAD-ZONE ring → FLOAT (in-window tear-off) ─────────
        _section("C: drag a tab into outliner's DEAD-ZONE → FLOAT (in-window)")
        # x_rel ~0.27 of outliner: past the 0.22 edge band, off the centre square →
        # the dead-zone ring → FLOAT, WITHOUT leaving the window.
        cx = out_r["x"] + 0.27 * out_r["w"]
        cy = out_r["y"] + 0.5 * out_r["h"]
        _drag_tab_to(tf, well, cx, cy)
        wait_until(lambda: "torn-properties" in _torn(tf), desc="C.1 a dead-zone release floated the tab")
        assert "torn-properties" in _torn(tf), "C.2 the tab floated from INSIDE the window"
        assert "properties" not in _docked(_topology(tf)), "C.3 properties left the dock"
        assert_eq(_wells(_topology(tf)), 0, "C.4 the well collapsed on the float")
        # The headline contrast: the cursor never left the window (cx,cy are inside
        # outliner) yet the tab floated — impossible under the continuous model.
        assert 0 <= cx <= MW and 0 <= cy <= MH, "C.5 the release point was INSIDE the window"
        assert_eq(len(_torn(tf)), 1, "C.6 exactly one floater from the in-window tear-off")
        assert {"toolbar", "outliner", "viewport", "console"} <= _docked(_topology(tf)), (
            "C.7 the other four panels stayed docked"
        )

        # ── (D) CENTRE → DOCK (tabify), no new floater ──────────────
        _section("D: drag a tab onto a panel CENTRE → DOCK (tabify)")
        # properties is floating now; tabify two STILL-DOCKED panels and centre-drop
        # one of their tabs onto a third panel (no dock-back juggling needed).
        tf.invoke(f"{REORG}/reorganize", {"source": "outliner", "target": "console", "zone": "Center"})
        well = wait_until(lambda: _well_id(tf), desc="D.1 a fresh well of docked panels")
        vp = _rect(tf, "viewport")
        assert vp, "D.2 viewport rect resolves"
        torn_before = _torn(tf)
        # centre of viewport = tabify (a deliberate dock, not a float).
        _drag_tab_to(tf, well, vp["x"] + 0.5 * vp["w"], vp["y"] + 0.5 * vp["h"])
        assert_eq(_torn(tf), torn_before, "D.3 a centre dock made no new floater")
        assert "viewport" in _well_members(_topology(tf)), "D.4 the tab tabified WITH viewport"
        assert "torn-properties" in _torn(tf), "D.5 the earlier in-window floater is unaffected"
        assert _wells(_topology(tf)) >= 1, "D.6 at least one well exists after the tabify"

        # ── (E) determinism ────────────────────────────────────────
        _section("E: determinism")
        assert_eq(_torn(tf), _torn(tf), "E.1 floater set stable")
        assert_eq(_wells(_topology(tf)), _wells(_topology(tf)), "E.2 well count stable")
        assert _topology(tf) is not None, "E.3 topology queryable as scene-as-data"
        assert_eq(_docked(_topology(tf)), _docked(_topology(tf)), "E.4 docked set stable")
        assert_eq(_well_members(_topology(tf)), _well_members(_topology(tf)), "E.5 well members stable")

        print("[demo] r1159_dock_drop_resolution: all sections PASS (dead-zone floats in-window)")


def _well_members(node: Any) -> set:
    out: set = set()
    if isinstance(node, dict):
        for p in node.get("panels", []):
            out.add(p)
        for v in node.values():
            out |= _well_members(v)
    elif isinstance(node, list):
        for x in node:
            out |= _well_members(x)
    return out


def _docked(node: Any, out: set | None = None) -> set:
    out = set() if out is None else out
    if isinstance(node, dict):
        if node.get("type") == "Leaf" and node.get("panel_id"):
            out.add(node["panel_id"])
        for p in node.get("panels", []):
            out.add(p)
        for v in node.values():
            _docked(v, out)
    elif isinstance(node, list):
        for x in node:
            _docked(x, out)
    return out


if __name__ == "__main__":
    sys.exit(run_demo("r1159_dock_drop_resolution", body))
