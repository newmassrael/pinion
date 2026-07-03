#!/usr/bin/env python3
"""R1201 §5.51 §2 #7 — suppress a REDUNDANT same-window OUTER full-span dock.

SCOPE — read this before the assertions. R1167 made a same-window dock-panel
drag near a window edge resolve to a full-span `OuterDock`. But the resolver
offered that outcome for EVERY near-edge cursor, including one over the dragged
panel's OWN full-span edge — so dragging the right column to the right edge (or
picking an edge-flush panel up and dropping it back) showed a full-span preview
and, on release, REMOVED + re-split the panel to the thin OUTER_DOCK_NEW_FRAC
band. That is a redundant no-op that silently resized the layout — the user's
"두번째 창을 오른쪽 끝에 댄 경우, 전폭 preview가 보이면 안되고 동작하면 안된다".

R1201 restores the VS Code / Qt ADS invariant: a drop indicator is offered only
when the outcome DIFFERS from the current layout. `resolve_drop_checked` threads
a redundancy predicate (`DockTopology::outer_dock_is_redundant`, a ratio/id-
agnostic structural compare of the would-be tree vs the current one); when the
dragged panel already occupies that full-span edge, the perimeter drop previews
+ resolves as a stay-put SnapBack — no band, no resize. `dock_panel_outer`
no-ops the same case for the RPC / §2 #2 path.

This drives it with real same-window `scene/drag`s and observes §2 #7 scene-as-data:

  (A) Boot — 5 panels, no reorg dividers.
  (B) Make the outliner a FULL-WIDTH bottom row (the R1167 outer dock — the
      non-regression baseline: a MEANINGFUL outer dock still works).
  (C) Re-drag the outliner to the BOTTOM edge it now spans → the preview is NOT
      the outer sentinel (redundant → SnapBack) and the release leaves the
      topology byte-identical (no resize, no re-minted split).
  (D) Contrast — drag the outliner to the RIGHT edge (which it does NOT occupy)
      → the preview IS the outer sentinel + a real move lands it as a right
      column. Non-tautological: same panel, same near-edge mechanism, only the
      OCCUPIED edge is suppressed.
  (E) Re-drag the outliner to the RIGHT edge it now spans → suppressed again.
  (F) Integrity + determinism.

The live FEEL is HW-gated; this pins what is observable as scene-as-data.
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


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in (tf.request("scene/windows", {}).result.get("windows") or [])}


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
        _section("A: boot — 5 panels, no reorg dividers")
        wins = tf.request("scene/windows", {}).result.get("windows") or []
        assert len(wins) == 1, f"A.1 one window at boot; got {wins!r}"
        assert wins[0].get("id") == _MAIN, f"A.2 the window is main ({wins[0]!r})"
        assert _panel_set(tf) == _ALL_PANELS, f"A.3 all 5 panels present ({_panel_set(tf)})"

        # ── (B) make the outliner a FULL-WIDTH bottom row (R1167) ─────
        _section("B: outer-dock the outliner to a FULL-WIDTH bottom row (baseline works)")
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="begin")
        prev = _drop_preview(tf, _OUTLINER)
        assert isinstance(prev, dict), f"B.1 a held drag in the bottom band has a preview ({prev!r})"
        assert _is_outer(prev.get("target")), f"B.2 ★a MEANINGFUL outer dock previews the sentinel ({prev.get('target')!r})"
        assert prev.get("zone") == "Bottom", f"B.3 the previewed edge is Bottom ({prev.get('zone')!r})"
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="end")
        wait_until(
            lambda: _rect(tf, _OUTLINER)["w"] >= _MAIN_W - 8,
            desc="B.4 the outliner now spans the full width",
        )
        row = _rect(tf, _OUTLINER)
        assert row["w"] >= _MAIN_W - 8, f"B.5 ★it spans the full width (w={row['w']})"
        assert row["y"] >= _MAIN_H * 0.5, f"B.6 it is the BOTTOM row (y={row['y']})"
        assert _panel_set(tf) == _ALL_PANELS, "B.7 the outer dock is a move — no panel lost"

        # ── (C) re-drag to the SAME (bottom) edge → suppressed ────────
        _section("C: re-drag the outliner to the bottom it already spans → NO outer, no change")
        topo_before = _topology(tf)
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="begin")
        cprev = _drop_preview(tf, _OUTLINER)
        assert not _is_outer(cprev.get("target") if isinstance(cprev, dict) else cprev), (
            f"C.1 ★no full-span preview when the panel already spans that edge ({cprev!r})"
        )
        assert cprev is None or not isinstance(cprev, dict), (
            f"C.2 ★a redundant outer drop previews NOTHING (a stay-put snap-back) ({cprev!r})"
        )
        assert _window_ids(tf) == {_MAIN}, "C.3 the redundant held drag spawns no floater (not a float)"
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="end")
        topo_after = _topology(tf)
        assert topo_after == topo_before, "C.4 ★the topology is byte-identical (no resize, no re-minted split)"
        still = _rect(tf, _OUTLINER)
        assert still["w"] >= _MAIN_W - 8, f"C.5 the outliner is still the full-width bottom row (w={still['w']})"
        assert _panel_set(tf) == _ALL_PANELS, "C.6 no panel lost to the redundant drop"

        # ── (D) contrast — drag to the RIGHT edge it does NOT occupy ──
        _section("D: drag the outliner to the RIGHT edge (a real move) → outer preview + right column")
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(1190.0, 400.0), phase="begin")
        dprev = _drop_preview(tf, _OUTLINER)
        assert isinstance(dprev, dict), f"D.1 a held drag in the right band has a preview ({dprev!r})"
        assert _is_outer(dprev.get("target")), (
            f"D.2 ★an UNoccupied edge still previews the outer sentinel — only the occupied edge is suppressed ({dprev!r})"
        )
        assert dprev.get("zone") == "Right", f"D.3 the previewed edge is Right ({dprev.get('zone')!r})"
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(1190.0, 400.0), phase="end")
        wait_until(
            lambda: _rect(tf, _OUTLINER)["h"] >= 600,
            desc="D.4 the outliner now spans the full height (a right column)",
        )
        col = _rect(tf, _OUTLINER)
        assert col["x"] + col["w"] >= _MAIN_W - 8, f"D.5 it is flush right ({col!r})"
        assert col["h"] >= 600, f"D.6 ★it spans the full height (h={col['h']})"
        assert col["w"] < 360, f"D.7 a thin right column, not a half-window (w={col['w']})"
        assert _panel_set(tf) == _ALL_PANELS, "D.8 still a move — no panel lost"

        # ── (E) re-drag to the SAME (right) edge → suppressed ─────────
        _section("E: re-drag the outliner to the right it now spans → suppressed again")
        topo_before_e = _topology(tf)
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(1190.0, 400.0), phase="begin")
        eprev = _drop_preview(tf, _OUTLINER)
        assert not _is_outer(eprev.get("target") if isinstance(eprev, dict) else eprev), (
            f"E.1 ★no full-span preview for the now-occupied RIGHT edge ({eprev!r})"
        )
        assert eprev is None or not isinstance(eprev, dict), f"E.2 ★redundant right dock previews nothing ({eprev!r})"
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(1190.0, 400.0), phase="end")
        assert _topology(tf) == topo_before_e, "E.3 ★the topology is unchanged by the redundant right drop"
        assert _rect(tf, _OUTLINER)["h"] >= 600, "E.4 the outliner is still the full-height right column"

        # ── (F) integrity + determinism ──────────────────────────────
        _section("F: integrity — panels survive, reads deterministic")
        assert _window_ids(tf) == {_MAIN}, "F.1 only main remains (no spurious floater)"
        assert _panel_set(tf) == _ALL_PANELS, "F.2 all 5 panels intact after every move"
        a = _topology(tf)
        b = _topology(tf)
        assert a == b, "F.3 back-to-back topology reads are identical"

        print("[demo] r1201_outer_dock_redundancy: all sections PASS (redundant outer dock suppressed)")


if __name__ == "__main__":
    sys.exit(run_demo("r1201_outer_dock_redundancy", body))
