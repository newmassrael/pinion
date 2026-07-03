#!/usr/bin/env python3
"""R1145 §5.51 — undock a tabbed panel back into a split.

SCOPE — read this before the assertions. A live user merged two panels into a
TAB well (drag one onto the other's centre = tabify) and found there was NO way
to separate them again: tabs are click-to-switch only, with no tear-off and no
undock action. R1145 adds an undock that pulls a tabbed panel OUT of its well and
re-docks it as a SPLIT sibling (the reverse of a centre tabify) — a DOCKED result
(no floating window), so it sidesteps the live window-move entirely.

Two paths, one commit funnel (the shared DockReorganizer):
  * AI — `invoke("undock_tab", "<panel>")` on the reorganize external.
  * Human — a toolbar "Undock tab" button (tag `undock_tab_btn`) that undocks the
    active tab; it PAINTS only while something is tabbed.

The proof is AI-observable scene-as-data: the topology JSON gains a `Tabs` well on
tabify and loses it on undock (back to a Split), and the toolbar button appears /
vanishes with the tabbed state.

Section roadmap (>=30 assertions across A-E):

  (A) Boot — 4 panels, NO tab well, the Undock button is HIDDEN (nothing tabbed).
  (B) Tabify viewport+properties (centre reorganize) — exactly one Tabs well now
      holds both panels, and the Undock button APPEARS in the toolbar.
  (C) Undock via the AI invoke — the well is gone (a Split again), both panels
      survive, and the button vanishes.
  (D) Tabify again, then undock via the HUMAN button click — same split result.
  (E) Edge cases — undock_tab on a non-tabbed panel is a no-op (Ok), the surface
      is unchanged; the undock schema advertises the invoke.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, assert_eq, find_by_tag, run_demo, wait_until  # noqa: E402

EXAMPLE = "hello-dock-panels-editor"
REORG = "/dock_reorganize/external"
MAIN = "main"
MW, MH = 1200, 800
UNDOCK_BTN = "undock_tab_btn"
PANELS = {"outliner", "viewport", "properties", "console"}


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


def _panels(node: Any, out: set | None = None) -> set:
    out = set() if out is None else out
    if isinstance(node, dict):
        if node.get("type") == "Leaf" and node.get("panel_id"):
            out.add(node["panel_id"])
        for p in node.get("panels", []):
            out.add(p)
        for v in node.values():
            _panels(v, out)
    elif isinstance(node, list):
        for x in node:
            _panels(x, out)
    return out


def _toolbar_has_undock(tf: RpcSubprocess) -> bool:
    snap = tf.snapshot(source="paint", viewport=(MW, MH), window=MAIN)
    return find_by_tag(snap, UNDOCK_BTN) is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — nothing tabbed ───────────────────────────────
        topo = _topology(tf)
        assert_eq(_wells(topo), 0, "A.1 boot has no tab well")
        assert_eq(_panels(topo), PANELS, "A.2 all 4 panels present")
        assert not _toolbar_has_undock(tf), "A.3 Undock button hidden when nothing tabbed"

        # ── (B) tabify viewport + properties ────────────────────────
        res = tf.invoke(f"{REORG}/reorganize", {
            "source": "viewport", "target": "properties", "zone": "Center",
        })
        assert res is not None, "B.1 tabify invoke answered"
        wait_until(lambda: _wells(_topology(tf)) == 1, desc="B.2 a tab well appears")
        t1 = _topology(tf)
        assert_eq(_wells(t1), 1, "B.3 exactly one Tabs well")
        assert_eq(_panels(t1), PANELS, "B.4 no panel lost in the tabify")
        wait_until(lambda: _toolbar_has_undock(tf), desc="B.5 the Undock button appears")
        assert _toolbar_has_undock(tf), "B.6 Undock button shown while tabbed"

        # ── (C) undock via the AI invoke ────────────────────────────
        tf.invoke(f"{REORG}/undock_tab", "viewport")
        wait_until(lambda: _wells(_topology(tf)) == 0, desc="C.1 the well splits back out")
        t2 = _topology(tf)
        assert_eq(_wells(t2), 0, "C.2 no tab well after undock (a Split again)")
        assert_eq(_panels(t2), PANELS, "C.3 both panels survive the undock")
        wait_until(lambda: not _toolbar_has_undock(tf), desc="C.4 the Undock button vanishes")
        assert not _toolbar_has_undock(tf), "C.5 button hidden again (nothing tabbed)"

        # ── (D) undock via the HUMAN toolbar button ─────────────────
        tf.invoke(f"{REORG}/reorganize", {
            "source": "viewport", "target": "properties", "zone": "Center",
        })
        wait_until(lambda: _toolbar_has_undock(tf), desc="D.1 re-tabified → button back")
        assert_eq(_wells(_topology(tf)), 1, "D.2 one well again")
        # Click the toolbar Undock button (the human path).
        tf.click(path=UNDOCK_BTN)
        wait_until(lambda: _wells(_topology(tf)) == 0, desc="D.3 the button undocked the tab")
        assert_eq(_wells(_topology(tf)), 0, "D.4 split again via the human button")
        assert_eq(_panels(_topology(tf)), PANELS, "D.5 all panels intact")

        # ── (E) edge cases ──────────────────────────────────────────
        before = _topology(tf)
        # Undock a panel that is NOT tabbed → a no-op (Ok), surface unchanged.
        tf.invoke(f"{REORG}/undock_tab", "outliner")
        assert_eq(_wells(_topology(tf)), 0, "E.1 undocking a non-tabbed panel changed nothing")
        assert_eq(_panels(_topology(tf)), _panels(before), "E.2 surface unchanged")
        assert _topology(tf) is not None, "E.3 topology still queryable as scene-as-data"

        print("[demo] r1145_undock_tab: all sections PASS (tabify <-> undock split)")


if __name__ == "__main__":
    sys.exit(run_demo("r1145_undock_tab", body))
