#!/usr/bin/env python3
"""R1146 §5.51 §2 #2 §2 #7 — the VS Code model: preview during the drag, real
window only on RELEASE.

SCOPE — read this before the assertions. The pre-R1146 floater drag was
APP-DRIVEN: it created + repositioned a real OS window on EVERY cursor move (the
`tear_off_follow` / `window_move` flood). The first escaped move did an expensive
GPU-surface init (the FREEZE) and a fast manual drag flooded the WM with
`set_outer_position` (the live HANG). The redesign moves the window mutation to
RELEASE-ONLY: during the drag only the LIGHTWEIGHT preview shows (the shell's
drag-image ghost + dock-zone guides + redock hint, all driven from the router's
live drag session), and the floating window is created ONCE on release. See
`docs/dock-window-move-redesign.md`.

This is the headless-observable proof of the architectural change, made possible
by the R1138 phased/held `scene/drag` (§2 #2): under a HELD mid-drag the declared
windows (`scene/windows`) show NO floater — it only appears once the drag is
RELEASED. The pre-R1146 per-move follow would have grown a `torn-` window on the
first escaped move, mid-hold. The live no-hang / no-freeze FEEL is HW-gated (a
real `:0` desktop); this demo pins what is observable as scene-as-data.

Driver: `hello-dock-panels` (the flat dock demo, the R1094 tear-off consumer).
Each tear-off drags a panel header to a point OUTSIDE the 880px window (over no
tagged region → the drag escapes the dock).

Section roadmap (>=30 assertions across A-F):

  (A) Boot — one WM-placed "main", all three panels docked (trees present), none
      detached, drag_cursor null.
  (B) HELD tear-off begin (the headline) — press the inspector header, march OUT
      (escaped), HOLD. The inspector reports detached=true + the held cursor, BUT
      `scene/windows` still lists ONLY "main" (NO floater mid-drag — release-only)
      AND the main dock STILL paints the inspector tree (the panel has not floated;
      the leaf stays live during the drag — the VS Code "ghost follows, panel
      stays" model). The held frame renders.
  (C) HELD move — re-aim the held drag to another escaped point: STILL no floater,
      STILL detached, the tree STILL painted, drag_cursor re-aimed.
  (D) END — release: NOW `torn-inspector` appears (created ONCE, at the release
      cursor), and the main dock swaps to the placeholder (the panel floated on
      release).
  (E) Atomic contrast — a plain `phase:"full"` tear-off of the property panel
      still floats it in one call (the release-only change preserves the atomic
      end state; only the per-move flood is gone).
  (F) Integrity — a 2nd HELD cycle shows no mid-drag window again, releases to a
      floater, and an unknown phase rejects loudly (no silent full-arc drag).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_MAIN_W = 880
_MAIN_H = 600
_MAIN = "main"

_INSPECTOR = "inspector"
_PROPERTY = "property"
_VIEWPORT = "viewport"
_PANELS = (_INSPECTOR, _PROPERTY, _VIEWPORT)
_HEADER = {p: f"{p}#header" for p in _PANELS}
_INSPECTOR_TREE = "inspector_tree"  # the inspector's content tag (a docked witness)
_FLOATING_PREFIX = "torn-"


def _torn(panel: str) -> str:
    return f"{_FLOATING_PREFIX}{panel}"


# ─── helpers ─────────────────────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    ws = resp.result.get("windows")
    assert isinstance(ws, list), f"scene/windows.windows must be a list; got {resp.result!r}"
    return ws


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in _windows(tf)}


def _window_by_id(tf: RpcSubprocess, window_id: str) -> Any:
    for w in _windows(tf):
        if w.get("id") == window_id:
            return w
    return None


def _detached(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/detached")


def _drag_cursor(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/drag_cursor")


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


def _assert_xy(label: str, value: Any, expected: tuple[int, int]) -> None:
    assert isinstance(value, list) and len(value) == 2, f"{label} must be [x, y]; got {value!r}"
    assert int(value[0]) == expected[0] and int(value[1]) == expected[1], (
        f"{label} must equal {expected}; got {value!r}"
    )


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    insp_to = (1040.0, 200.0)  # escaped (x past the 880px width)
    insp_reaim = (1080.0, 360.0)
    prop_to = (1060.0, 420.0)
    vp_to = (1020.0, 480.0)

    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) boot — single main, panels docked ────────────────────
        _section("A: boot — single 'main', three panels docked")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 boot declares one window; got {boot!r}"
        assert boot[0].get("id") == _MAIN, f"A.2 the boot window is 'main'; got {boot[0]!r}"
        assert boot[0].get("position") is None, f"A.3 main is WM-placed (null position); {boot[0]!r}"
        scene_a = _main_scene(tf)
        assert _scene_contains_tag(scene_a, _INSPECTOR_TREE), "A.4 the inspector tree is docked (painted)"
        for panel in _PANELS:
            assert not _scene_contains_tag(scene_a, f"{panel}_placeholder"), (
                f"A.5 {panel} shows no placeholder while docked"
            )
            assert _detached(tf, panel) is False, f"A.6 {panel} not detached at boot"
            assert _drag_cursor(tf, panel) is None, f"A.7 {panel} drag_cursor null at boot"

        # ── (B) HELD tear-off begin — NO floater mid-drag ────────────
        _section("B: HELD tear-off begin — escaped + held, NO floater created")
        tf.drag(from_path=_HEADER[_INSPECTOR], to_at=insp_to, steps=8, phase="begin")
        wait_until(lambda: _detached(tf, _INSPECTOR) is True, desc="B.1 the held drag escapes (detached)")
        _assert_xy("B.2 inspector drag_cursor (held)", _drag_cursor(tf, _INSPECTOR), (1040, 200))
        # ★ THE HEADLINE: a held escaped drag has created NO floating window.
        assert _window_ids(tf) == {_MAIN}, (
            f"B.3 release-only: NO floater while the drag is HELD; got {_window_ids(tf)!r}"
        )
        assert _window_by_id(tf, _torn(_INSPECTOR)) is None, "B.4 no torn-inspector mid-drag"
        # ★ The panel has NOT floated — its leaf stays live (VS Code model: the
        #   original stays, the shell's ghost follows). The tree is still painted.
        scene_b = _main_scene(tf)
        assert _scene_contains_tag(scene_b, _INSPECTOR_TREE), (
            "B.5 the inspector tree is STILL painted mid-drag (the panel has not floated)"
        )
        assert not _scene_contains_tag(scene_b, "inspector_placeholder"), (
            "B.6 no placeholder yet (the panel has not floated mid-drag)"
        )

        # ── (C) HELD move — re-aim, STILL no floater ─────────────────
        _section("C: HELD move — re-aim the held drag, STILL no floater")
        tf.drag(from_at=insp_to, to_at=insp_reaim, steps=6, phase="move")
        wait_until(
            lambda: _drag_cursor(tf, _INSPECTOR) == [int(insp_reaim[0]), int(insp_reaim[1])],
            desc="C.1 the move re-aims the held drag_cursor",
        )
        assert _detached(tf, _INSPECTOR) is True, "C.2 still detached (held, not released)"
        assert _window_ids(tf) == {_MAIN}, (
            f"C.3 still NO floater after a held MOVE (no per-move window); got {_window_ids(tf)!r}"
        )
        assert _scene_contains_tag(_main_scene(tf), _INSPECTOR_TREE), (
            "C.4 the inspector tree is STILL painted (no window mutation on a move)"
        )

        # ── (D) END — release creates the floater ONCE ───────────────
        _section("D: END — release creates the floater ONCE at the release cursor")
        tf.drag(from_at=insp_reaim, to_at=insp_reaim, steps=0, phase="end")
        entry = wait_until(
            lambda: _window_by_id(tf, _torn(_INSPECTOR)),
            desc="D.1 the floater appears ONLY after release",
        )
        assert _window_ids(tf) == {_MAIN, _torn(_INSPECTOR)}, (
            f"D.2 exactly one floater after release; got {_window_ids(tf)!r}"
        )
        _assert_xy("D.3 floater opened at the release cursor", entry.get("position"), (1080, 360))
        assert _detached(tf, _INSPECTOR) is True, "D.4 the inspector floated (detached)"
        scene_d = _main_scene(tf)
        assert not _scene_contains_tag(scene_d, _INSPECTOR_TREE), (
            "D.5 the inspector tree left main (the panel floated on release)"
        )
        assert _scene_contains_tag(scene_d, "inspector_placeholder"), (
            "D.6 main now paints the inspector placeholder"
        )

        # ── (E) atomic contrast — full drag floats in one call ───────
        _section("E: atomic full tear-off — the end state is preserved")
        tf.drag(from_path=_HEADER[_PROPERTY], to_at=prop_to, steps=8, phase="full")
        wait_until(lambda: _window_by_id(tf, _torn(_PROPERTY)), desc="E.1 the full drag floats property")
        ids_e = _window_ids(tf)
        assert _torn(_PROPERTY) in ids_e and _torn(_INSPECTOR) in ids_e, "E.2 both floaters declared"
        assert _detached(tf, _PROPERTY) is True, "E.3 property detached after the full drag"
        pos_i = _window_by_id(tf, _torn(_INSPECTOR)).get("position")
        pos_p = _window_by_id(tf, _torn(_PROPERTY)).get("position")
        assert pos_i != pos_p, f"E.4 the two floaters hold distinct positions ({pos_i} vs {pos_p})"

        # ── (F) integrity — 2nd held cycle + unknown-phase rejection ─
        _section("F: 2nd held cycle (no mid-drag window) + unknown phase rejects")
        tf.drag(from_path=_HEADER[_VIEWPORT], to_at=vp_to, steps=8, phase="begin")
        wait_until(lambda: _detached(tf, _VIEWPORT) is True, desc="F.1 the 2nd held drag escapes")
        assert _window_by_id(tf, _torn(_VIEWPORT)) is None, (
            "F.2 STILL no floater mid-hold on the 2nd cycle (release-only holds)"
        )
        tf.drag(from_at=vp_to, to_at=vp_to, steps=0, phase="end")
        wait_until(lambda: _window_by_id(tf, _torn(_VIEWPORT)), desc="F.3 the 2nd release creates its floater")
        assert _torn(_VIEWPORT) in _window_ids(tf), "F.4 the viewport floater is declared after release"
        # An out-of-vocabulary phase rejects loudly (the R1138 wire-vocab guard).
        raised = False
        try:
            tf.request("scene/drag", {"from": {"x": 10.0, "y": 10.0}, "to": {"x": 50.0, "y": 50.0}, "phase": "hover"})
        except RpcError as exc:
            raised = exc.code != 0
        assert raised, "F.5 an unknown phase is rejected (invalid_params)"

        print("[demo] r1146_release_only_window_move: all sections PASS (VS Code model — window on release only)")


if __name__ == "__main__":
    sys.exit(run_demo("r1146_release_only_window_move", body))
