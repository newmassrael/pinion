#!/usr/bin/env python3
"""R1103 §5.51 §2 #7 PR-33 — the cross-window redock EXECUTES (mutation slice 3).

SCOPE — read this before the assertions. R1100-R1102 made a dragged panel FIRE a
cross-window redock intent + recorded it as an AI-observable diagnostic
(`query("redock_at")`), but the intent had NO reducer consumer: firing it moved
no panel (R1102's section G asserted exactly that fire-only boundary). R1103
gives it a consumer, so a floating panel returning into the slot-bearing window
actually RE-DOCKS — an observable topology change, not just a diagnostic.

The DRIVER is an AI-primary `invoke("tear_off_redock_at", {window, target, x_rel,
y_rel})` channel, NOT a live pointer drag. The live floater→main drag is blocked:
the source floating window follows the cursor (R1094 `tear_off_follow`), and the
R1095 moving-source desktop-conversion carry that would reconcile that has not
landed, so the abs cursor double-counts the moving origin. Driving the redock
through `invoke` sidesteps the live pointer (the same bypass the R683.C `tear_off`
dock-back invoke uses) and is the AI-first path regardless.

The EXECUTABLE case is floater→main. `hello-dock-panels` is a flat splitter
layout: each panel has a fixed dock slot, and a torn-off panel shows a
placeholder in its slot while its content lives in a floating single-panel
window. Only the slot-bearing "main" window can host a panel, so a redock INTO
main executes (the floating window is removed → the slot re-installs the real
panel); a redock targeting another floater (no slot to host) FIRES + is observable
but executes no move. Zone-specific placement (honouring target/x_rel/y_rel into a
non-home slot) is the flat-reducer dock-at-zone of slice 4.

Section roadmap (>=30 assertions across A-F):

  (A) Boot — one WM-placed "main", three panels docked (real tags, no
      placeholders), every redock_at null, none detached.
  (B) Tear off TWO panels (inspector + property) via the toggle invoke — three
      declared windows, both slots show placeholders, both real tags gone.
  (C) The mixed invoke — the headline + the boundary together. Invoke a redock
      of inspector onto a NON-main window (another floater, no slot) AND a redock
      of property onto MAIN. Each `scene/invoke` drains + reduces its OWN intent
      synchronously in that RPC's tail, so once both invokes have returned both
      have reduced: the inspector redock no-oped (its floater untouched) and
      property's executed (its floater gone, window count 3 -> 2). The proof the
      inspector was REJECTED (not merely un-run) is firing-witness + survival:
      its redock_at recorded the non-main target it fired (C.8/C.9) AND its
      floater SURVIVES property's execution — not a timing race.
  (D) Content restored — main hosts the real property panel again (placeholder
      gone), while the inspector slot still shows its placeholder (still floating).
  (E) Read-only — `scene/intervene` on redock_at is rejected (router-driven).
  (F) Accept-after-reject — redock the inspector onto MAIN: now it executes too
      (window count -> one, real inspector restored). The same panel that was
      rejected for a non-main target in C succeeds for main, so the guard is a
      target discriminator, not a panel block.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_INSPECTOR = "inspector"
_PROPERTY = "property"
_VIEWPORT = "viewport"
_PANELS = (_INSPECTOR, _PROPERTY, _VIEWPORT)
_FLOATING_PREFIX = "torn-"
_MAIN = "main"
_MAIN_W = 880
_MAIN_H = 600


# ─── helpers ─────────────────────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    ws = resp.result.get("windows")
    assert isinstance(ws, list), f"scene/windows.windows must be a list; got {resp.result!r}"
    return ws


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in _windows(tf)}


def _torn(panel: str) -> str:
    return f"{_FLOATING_PREFIX}{panel}"


def _main_scene(tf: RpcSubprocess) -> Any:
    return tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window=_MAIN)


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


def _placeholder(panel: str) -> str:
    return f"{panel}_placeholder"


def _redock_at(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/redock_at")


def _detached(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/detached")


def _tear_off(tf: RpcSubprocess, panel: str) -> None:
    """Float `panel` via the R683.C toggle invoke (deterministic — no drag
    geometry; the floater opens at the per-panel cascade position)."""
    tf.invoke(f"/{panel}/external/tear_off", None)


def _redock_into(tf: RpcSubprocess, panel: str, window: str, *, target: str) -> None:
    """R1103 AI-primary cross-window redock driver: redock `panel` into
    `window`'s dock zone `target` (zone centre)."""
    tf.invoke(
        f"/{panel}/external/tear_off_redock_at",
        {"window": window, "target": target, "x_rel": 0.5, "y_rel": 0.2},
    )


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) boot — one window, all panels docked, no redock yet ──
        _section("A: boot — single window, three panels docked")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 boot declares one window; got {boot!r}"
        assert boot[0].get("id") == _MAIN, f"A.2 the boot window is 'main'; got {boot[0]!r}"
        assert boot[0].get("position") is None, f"A.3 main is WM-placed (null position); {boot[0]!r}"
        scene = _main_scene(tf)
        for panel in _PANELS:
            assert _scene_contains_tag(scene, panel), f"A.4 {panel} is docked (real tag present)"
            assert not _scene_contains_tag(scene, _placeholder(panel)), (
                f"A.5 {panel} shows no placeholder while docked"
            )
            assert _redock_at(tf, panel) is None, f"A.6 {panel} redock_at null before any redock"
            assert _detached(tf, panel) is False, f"A.7 {panel} not detached at boot"

        # ── (B) tear off inspector + property → three windows ────────
        _section("B: tear off inspector + property (two floaters)")
        _tear_off(tf, _INSPECTOR)
        _tear_off(tf, _PROPERTY)
        wait_until(
            lambda: _torn(_INSPECTOR) in _window_ids(tf) and _torn(_PROPERTY) in _window_ids(tf),
            desc="B.1 both floating windows appear",
        )
        ids = _window_ids(tf)
        assert len(ids) == 3, f"B.2 three declared windows after two tear-offs; got {ids!r}"
        assert {_MAIN, _torn(_INSPECTOR), _torn(_PROPERTY)} == ids, f"B.3 the expected windows ({ids!r})"
        scene_b = _main_scene(tf)
        for panel in (_INSPECTOR, _PROPERTY):
            assert _scene_contains_tag(scene_b, _placeholder(panel)), (
                f"B.4 main shows {panel}'s placeholder while floating"
            )
            assert not _scene_contains_tag(scene_b, panel), f"B.5 main no longer hosts the real {panel}"
        assert _scene_contains_tag(scene_b, _VIEWPORT), "B.6 the un-torn viewport stays docked"

        # ── (C) mixed invoke — non-main rejected, main executed ──────
        _section("C: redock inspector→non-main (rejected) + property→main (executed)")
        # Inspector onto ANOTHER floater (no dock slot to host it) — fires but
        # must execute nothing. Property onto MAIN — fires AND executes. Each
        # scene/invoke drains + reduces its OWN intent synchronously in that RPC's
        # tail, so once both invokes return both have reduced: inspector no-oped,
        # property removed its floater. The reject (vs un-run) is proven by
        # firing-witness + survival — C.8/C.9 assert inspector's redock_at fired
        # the non-main target, and torn-inspector SURVIVES property's execution.
        _redock_into(tf, _INSPECTOR, _torn(_PROPERTY), target=_INSPECTOR)
        _redock_into(tf, _PROPERTY, _MAIN, target=_VIEWPORT)
        wait_until(lambda: len(_windows(tf)) == 2, desc="C.1 the window count drops to two")
        settled = _window_ids(tf)
        assert _torn(_PROPERTY) not in settled, "C.2 property redocked into main (its floater is gone)"
        assert _torn(_INSPECTOR) in settled, (
            "C.3 the inspector floater SURVIVES — its non-main redock target was rejected, "
            "not executed (same drain cycle as property's execution)"
        )
        prop_at = _redock_at(tf, _PROPERTY)
        assert isinstance(prop_at, dict), f"C.4 property's redock_at recorded the firing ({prop_at})"
        assert prop_at["window"] == _MAIN, f"C.5 property's redock targeted main ({prop_at})"
        assert prop_at["panel"] == _PROPERTY, f"C.6 the payload names the property panel ({prop_at})"
        assert prop_at["target"] == _VIEWPORT, f"C.7 the recorded dock zone is the viewport ({prop_at})"
        insp_at = _redock_at(tf, _INSPECTOR)
        assert isinstance(insp_at, dict), f"C.8 inspector's redock_at ALSO fired ({insp_at})"
        assert insp_at["window"] == _torn(_PROPERTY), (
            f"C.9 inspector fired the non-main target it was rejected for ({insp_at})"
        )
        assert _detached(tf, _PROPERTY) is False, "C.10 the executed redock cleared property's latch"

        # ── (D) content restored for property; inspector still floats ─
        _section("D: main re-hosts property; inspector slot still a placeholder")
        scene_d = _main_scene(tf)
        assert _scene_contains_tag(scene_d, _PROPERTY), "D.1 the real property panel is back in main"
        assert not _scene_contains_tag(scene_d, _placeholder(_PROPERTY)), (
            "D.2 property's placeholder is gone (it re-docked)"
        )
        assert _scene_contains_tag(scene_d, _placeholder(_INSPECTOR)), (
            "D.3 inspector still shows its placeholder (the non-main redock did not move it)"
        )
        assert not _scene_contains_tag(scene_d, _INSPECTOR), "D.4 inspector's real panel is still floating"

        # ── (E) redock_at is read-only ───────────────────────────────
        _section("E: redock_at is a read-only diagnostic")
        try:
            tf.request("scene/intervene", {"path": f"/{_PROPERTY}/external/redock_at", "value": None})
            raise AssertionError("E.1 intervening on redock_at must error")
        except RpcError as exc:
            assert exc.code != 0, f"E.1 redock_at intervene rejected (code {exc.code})"

        # ── (F) accept-after-reject — inspector→main now executes ────
        _section("F: redock inspector→main — the same panel now executes")
        _redock_into(tf, _INSPECTOR, _MAIN, target=_PROPERTY)
        wait_until(lambda: len(_windows(tf)) == 1, desc="F.1 inspector redocks into main → one window")
        final = _window_ids(tf)
        assert final == {_MAIN}, f"F.2 only main remains ({final})"
        scene_f = _main_scene(tf)
        assert _scene_contains_tag(scene_f, _INSPECTOR), "F.3 the real inspector panel is back in main"
        assert not _scene_contains_tag(scene_f, _placeholder(_INSPECTOR)), (
            "F.4 inspector's placeholder is gone"
        )
        insp_final = _redock_at(tf, _INSPECTOR)
        assert isinstance(insp_final, dict) and insp_final["window"] == _MAIN, (
            f"F.5 inspector's last redock targeted main and executed ({insp_final})"
        )
        # The guard is a target discriminator (only the slot-bearing window
        # hosts), not a per-panel block: inspector was rejected for a non-main
        # target in C and accepted for main here.

        print("[demo] r1103_cross_window_redock_execute: all sections PASS (floater→main executes)")


if __name__ == "__main__":
    sys.exit(run_demo("r1103_cross_window_redock_execute", body))
