#!/usr/bin/env python3
"""R1348 §5.51 §2 #7 PR-57 — the outer-dock band stops CLAIMING a perimeter it will not use.

SCOPE — read this before the assertions. R1201 declared the VS Code / the toolkit ADS rule
that "a drop indicator is offered only when the outcome DIFFERS", but enforced it
only at RESOLVE: `resolve_drop_checked` mapped a redundant perimeter drop to a
stay-put SnapBack. The router's CLAIM (`InputRouter::resolve_own_outer_dock`) was
still UNCONDITIONAL — a cursor within the edge band minted the OUTER_DOCK_ZONE_TAG
sentinel *instead of* hit-testing the panel there. So the outcome died while the
claim survived, and the band became a DEAD STRIP: no preview, no action, AND the
ordinary split bands of the panel underneath were unreachable, because that
coordinate never reached a panel at all.

With exactly 2 PANE SLOTS every edge is redundant (R1338 — removing the dragged
one leaves a lone pane, so the outer band is an inner split at a worse ratio), so
the ENTIRE perimeter of a 2-pane dock was dead. That is the most common layout
there is (an IDE's editor+console, a terminal's left/right split) — the sprag
consumer report that prompted PR-57: "좌우 팬이 있는데 왜 모서리가 활성화돼?".

R1348 moves the rule UP to the claim: the router OFFERS the perimeter to the drag
source (`External::accepts_outer_dock`) before stealing the hit-test, and the dock
answers with the SAME live-topology predicate its release resolves with. A refused
band falls through to the plain hit-test — the band's interior path, no new
concept — so "claimed but inert" is now unrepresentable rather than merely
unwanted.

This drives it with real same-window `scene/drag`s and observes §2 #7 scene-as-data:

  (A) Boot — 4 dock panels (4 pane slots).
  (B) NON-REGRESSION: with 4 slots a near-edge drag still CLAIMS the perimeter,
      and the release still APPLIES the full-span outer dock (R1167 untouched —
      this PR narrows WHEN the band is claimed, it does not retire the band).
      This lands console as the left column, the shape (C)-(E) start from.
  (C) Collapse to 2 SLOTS (tabify 3 panels into one well; console stays a bare
      leaf beside it) — now every edge of console's perimeter is redundant.
  (D) ★THE FIX: a near-edge drag no longer previews the outer sentinel — it
      previews a REAL inner Dock on the panel beneath, which the dead band used
      to make unreachable. Driven at the RIGHT edge, i.e. over the tab well and
      NOT over console's own slot, so what is under test is the claim over a
      BYSTANDER panel rather than the R1162 self-drop snap-back.
  (E) ★And the release APPLIES it: the topology really changes. Pre-R1348 this
      exact gesture was a no-op (SnapBack) — the user's "아무 일도 일어나지 않는다".
  (F) Integrity + determinism.
  (G) ★The honest half of "the perimeter is just interior": the band and the pixel
      beside it AGREE — including where the shared answer is unwelcome. Over a
      tab-strip background both resolve Float and tear the panel off (R1158's
      deliberate non-panel rule). Pre-R1348 the band was inert there, so R1348 DOES
      change that coordinate; (G) pins that the change is the interior rule
      ARRIVING, not a perimeter-only special case. Runs on its own instances (see
      its docstring for why the main body cannot host it).

Both (D) and (E) were verified to FAIL against the pre-R1348 binary (the preview
read None — the dead band), so they pin the bug and not merely the fix.

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

_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_ALL_PANELS = {_OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}


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


def _slot_count(node: Any) -> int:
    """PANE SLOTS — a tab well is ONE slot however many panels it stacks
    (`DockNode::leaf_count`), the unit the R1338 redundancy rule counts in."""
    if not isinstance(node, dict):
        return 0
    kind = node.get("type")
    if kind in ("Leaf", "Tabs"):
        return 1
    if kind == "Split":
        return _slot_count(node.get("first")) + _slot_count(node.get("second"))
    return 0


def _panel_set(tf: RpcSubprocess) -> set[str]:
    return _all_panels(_topology(tf).get("root"))


def _slots(tf: RpcSubprocess) -> int:
    return _slot_count(_topology(tf).get("root"))


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


def _reorganize(tf: RpcSubprocess, source: str, target: str, zone: str) -> Any:
    return tf.invoke(f"{_REORG}/reorganize", {"source": source, "target": target, "zone": zone})


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        _section("A: boot — 4 dock panels = 4 pane slots")
        assert _panel_set(tf) == _ALL_PANELS, f"A.1 all 4 dock panels present ({_panel_set(tf)})"
        assert _slots(tf) == 4, f"A.2 four pane slots at boot ({_slots(tf)})"

        # ── (B) NON-REGRESSION — ≥3 slots still CLAIM the perimeter ───
        # A full-span band crossing every pane is an arrangement no single inner
        # split can reproduce, so it keeps its unique worth and stays claimed — and
        # the release still APPLIES it. This PR narrows WHEN the band is claimed; it
        # does not retire the band.
        _section("B: 4 slots — a near-edge drag STILL claims the outer band (R1167 intact)")
        tf.drag(from_path=f"{_CONSOLE}#header", to_at=(10.0, 300.0), phase="begin")
        bprev = _drop_preview(tf, _CONSOLE)
        assert isinstance(bprev, dict), f"B.1 a held drag in the left band has a preview ({bprev!r})"
        assert _is_outer(bprev.get("target")), (
            f"B.2 ★with ≥3 slots a MEANINGFUL outer dock still previews the sentinel ({bprev!r})"
        )
        assert bprev.get("zone") == "Left", f"B.3 the previewed edge is Left ({bprev.get('zone')!r})"
        # A release inside a CLAIMED band applies the outer dock (preview == result),
        # so this is a real move, not an abort — console becomes the full-height left
        # column, and the sections below start from that shape.
        tf.drag(from_path=f"{_CONSOLE}#header", to_at=(10.0, 300.0), phase="end")
        wait_until(
            lambda: _rect(tf, _CONSOLE)["h"] >= 600,
            desc="B.4 the console becomes a full-height left column",
        )
        col = _rect(tf, _CONSOLE)
        assert col["x"] < 8, f"B.5 ★the claimed outer dock APPLIED — flush left ({col!r})"
        assert col["h"] >= 600, f"B.6 ★and full-height (h={col['h']}) — the R1167 gesture works"
        assert _panel_set(tf) == _ALL_PANELS, "B.7 no panel lost"

        # ── (C) collapse to 2 SLOTS ──────────────────────────────────
        # Tabify 3 panels into ONE well (a well = one pane slot), leaving `console`
        # a bare leaf beside it: the 2-slot shape sprag reports, reached with pure
        # §2 #2 RPC (no coordinates), so the setup cannot be blamed for the result.
        _section("C: tabify 3 panels into one well → 2 pane slots (well | console)")
        _reorganize(tf, _OUTLINER, _VIEWPORT, "Center")
        _reorganize(tf, _PROPERTIES, _VIEWPORT, "Center")
        wait_until(lambda: _slots(tf) == 2, desc="C.1 the dock collapses to 2 pane slots")
        assert _slots(tf) == 2, f"C.2 ★exactly 2 pane slots ({_slots(tf)})"
        assert _panel_set(tf) == _ALL_PANELS, "C.3 tabify is a move — all 4 panels still present"

        # ── (D) ★THE FIX — the dead band is gone ─────────────────────
        # Drive the RIGHT edge: console now occupies the left column (B), and a drop
        # point over the dragged panel's OWN slot is a snap-back for reasons that
        # predate this PR (R1162) — which would prove nothing about the claim. The
        # right band sits over the tab well, i.e. over a panel that is NOT the drag
        # source, so what the band does to a BYSTANDER panel is exactly what is
        # under test.
        _section("D: 2 slots — a near-edge drag now reaches the panel beneath the band")
        topo_before = _topology(tf)
        tf.drag(from_path=f"{_CONSOLE}#header", to_at=(1190.0, 400.0), phase="begin")
        dprev = _drop_preview(tf, _CONSOLE)
        assert isinstance(dprev, dict), (
            f"D.1 ★the band is no longer DEAD — it previews something ({dprev!r}). Pre-R1348 the "
            f"claim stood and the outcome snapped back, so this read None."
        )
        assert not _is_outer(dprev.get("target")), (
            f"D.2 ★no outer sentinel: with 2 slots the outer dock is redundant, so the band is "
            f"not claimed at all ({dprev!r})"
        )
        assert dprev.get("target") in _ALL_PANELS, (
            f"D.3 ★the claim fell through to a REAL panel under the cursor — the split bands the "
            f"dead strip used to mask ({dprev.get('target')!r})"
        )
        assert dprev.get("source") == _CONSOLE, f"D.4 the dragged panel is console ({dprev!r})"
        assert dprev.get("target") != _CONSOLE, (
            f"D.5 the panel under the band is a BYSTANDER, not the drag source — so this is the "
            f"claim under test, not the R1162 self-drop snap-back ({dprev!r})"
        )
        assert dprev.get("zone") == "Right", (
            f"D.6 ★and it is that panel's own RIGHT split band — the ordinary inner gesture the "
            f"dead strip used to cover ({dprev.get('zone')!r})"
        )
        assert _window_ids(tf) == {_MAIN}, "D.7 the held drag spawns no floater (not a float)"

        # ── (E) ★the release APPLIES it (pre-R1348: a no-op) ─────────
        _section("E: release → the inner split really lands (the gesture was a no-op before)")
        target_panel = dprev.get("target")
        tf.drag(from_path=f"{_CONSOLE}#header", to_at=(1190.0, 400.0), phase="end")
        wait_until(
            lambda: _topology(tf) != topo_before,
            desc="E.1 the release changes the topology",
        )
        assert _topology(tf) != topo_before, (
            "E.2 ★the release APPLIED a real move — pre-R1348 the perimeter claim held this "
            "coordinate and resolved it to a stay-put SnapBack, so the drag did literally "
            "nothing: the user's '아무 일도 일어나지 않는다'"
        )
        console_rect = _rect(tf, _CONSOLE)
        target_rect = _rect(tf, target_panel)
        assert console_rect["x"] >= target_rect["x"] + target_rect["w"] - 8, (
            f"E.3 ★console landed on the RIGHT of {target_panel} — the previewed zone, applied "
            f"(console={console_rect!r} {target_panel}={target_rect!r})"
        )
        assert console_rect["w"] > _MAIN_W * 0.25, (
            f"E.4 ★a real inner split (~half), NOT the thin OUTER_DOCK_NEW_FRAC band — the "
            f"arrangement the outer dock could not give ({console_rect!r})"
        )
        assert _panel_set(tf) == _ALL_PANELS, "E.5 a move — no panel lost"

        # ── (F) integrity + determinism ──────────────────────────────
        _section("F: integrity — panels survive, reads deterministic")
        assert _window_ids(tf) == {_MAIN}, "F.1 only main remains (no spurious floater)"
        assert _panel_set(tf) == _ALL_PANELS, "F.2 all 4 dock panels intact after every move"
        a = _topology(tf)
        b = _topology(tf)
        assert a == b, "F.3 back-to-back topology reads are identical"

    # ── (G) ★the band AGREES with the pixel beside it ────────────────
    _band_agrees_with_the_interior_beside_it()

    print("[demo] r1348_outer_dock_claim_veto: all sections PASS (no dead perimeter band)")


def _band_agrees_with_the_interior_beside_it() -> None:
    """(G) ★"a vetoed perimeter is just interior" — measured, not asserted.

    Honest only if the band and the pixel beside it really resolve the same, INCLUDING
    where the shared answer is unwelcome: over a tab-strip background both resolve
    `Float` (a non-panel target — R1158's deliberate rule) and tear the panel off.
    Pre-R1348 the band was inert there, so R1348 DOES change that coordinate; what
    this pins is that the change is the interior rule ARRIVING, not a perimeter-only
    special case. Exempting the band from the float would rebuild the very
    perimeter-vs-interior asymmetry this round removes.

    Runs its own app instance on the PRISTINE 2-slot shape (the well on top, its tab
    strip spanning the full width). The main body cannot host this: by then console
    has been moved to the right half, so both probes would land on console's OWN slot
    and pass via the R1162 self-drop — the exact confound (D.5) this demo avoids.
    """
    _section("G: the vetoed band resolves the same as the interior beside it")
    strip_y = 60.0  # the tab strip: below the dock top (48), above the well body (96)
    probes = (
        ("in-band", (_MAIN_W - 4.0, strip_y)),  # <=32px from the right edge -> in band
        ("interior", (_MAIN_W - 60.0, strip_y)),  # 60px in -> never in the band
    )
    outcomes = []
    for label, pt in probes:
        # A fresh instance per probe: a tear-off is not trivially undoable, and each
        # probe must start from the identical pristine shape to be comparable.
        with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
            _reorganize(tf, _OUTLINER, _VIEWPORT, "Center")
            _reorganize(tf, _PROPERTIES, _VIEWPORT, "Center")
            wait_until(lambda: _slots(tf) == 2, desc=f"G.0 {label}: 2 pane slots")
            tf.drag(from_path=f"{_CONSOLE}#header", to_at=pt, phase="begin")
            preview = _drop_preview(tf, _CONSOLE)
            tf.drag(from_path=f"{_CONSOLE}#header", to_at=pt, phase="end")
            # Compare the OUTCOME, not just the preview: both previews are None here,
            # so preview-equality ALONE would be a tautology (None == None) that passes
            # even if the two coordinates did entirely different things.
            outcomes.append((preview, _window_ids(tf) != {_MAIN}))
            print(f"[demo]    {label:<9} {pt} -> preview={preview!r} tore_off={outcomes[-1][1]}")
    assert outcomes[0] == outcomes[1], (
        f"★G.1 the in-band and the interior AGREE on preview AND outcome over the same "
        f"widget — a vetoed perimeter is not a special case ({outcomes!r})"
    )
    assert outcomes[0][1], (
        f"★G.2 …and the shared outcome is specifically a TEAR-OFF (a non-panel target "
        f"resolves Float — R1158). Non-tautological: a real, unwelcome-looking outcome "
        f"the band now inherits, which the interior 60px away has always done ({outcomes!r})"
    )


if __name__ == "__main__":
    sys.exit(run_demo("r1348_outer_dock_claim_veto", body))
