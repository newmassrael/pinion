#!/usr/bin/env python3
"""R1105 §5.51 §5.16 §5.41 PR-31 — the EDITOR is the 2nd float-to-window consumer.

SCOPE — read this before the assertions. `hello-dock-panels-editor` is the
5-pane DCC/IDE editor built on a declarative `DockTopology` (the R685 2nd dock
consumer). Before R1105 it consumed the reorganizer (dock / split / tabify) but
had NO `windows_signal` and wired NO tear-off arm, so a panel could not float
into its own window (the R1094/R1095.1 "editor 2nd consumer would unlock this"
carry). R1105 wires it: a panel tears off into its own floating `WindowSpec`
(observable over `scene/windows`) while its dock LEAF stays in the topology and
paints a `view_floating_placeholder` — so the panel is in exactly one place (the
floating window), never duplicated, and dock-back is trivial (the slot is its
home). The driver is the AI-primary `invoke("tear_off")` toggle +
`invoke("tear_off_redock_at", {window, target, …})` (the §2 #2 RPC path; the live
floater→main drag is the parked moving-source-coord carry).

UNLIKE the flat `hello-dock-panels` walker (which swaps the WHOLE panel for the
placeholder when floating), the editor's `view_dock_surface` walker always wraps
each leaf in `view_dock_panel(panel_id, …)`, so the panel ROOT tag is always
present — docked-vs-floating is read off the panel's CONTENT-body tag
(`{panel}_content_body`, present only while docked) vs its placeholder tag
(`{panel}_placeholder`, present only while floating).

HONEST LIMIT (this slice): a redock into main returns the panel to its HOME slot
(its topology leaf, still present as a placeholder) — the `target`/`x_rel`/`y_rel`
zone is recorded (forward-looking) but NOT yet honored. Zone-HONORING redock
(relocating the panel in the `DockTopology` via the reorganizer — the editor's
distinguishing value over the flat demo) is R1106's slice-4(a), built on this
foundation.

Section roadmap (>=30 assertions across A-F):

  (A) Boot — one WM-placed "main" (1200x800), three demo panels docked (content
      bodies present, no placeholders), every redock_at null, none detached.
  (B) Tear off TWO panels (outliner + viewport) via the toggle invoke — three
      declared windows, both slots show placeholders, both content bodies gone.
  (C) Mixed invoke — the headline + the boundary together. Redock outliner onto a
      NON-main window (another floater, no slot) AND redock viewport onto MAIN.
      Each scene/invoke drains + reduces its OWN intent synchronously in that
      RPC's tail, so once both return both have reduced: outliner no-oped (its
      floater survives), viewport executed (its floater gone, 3 -> 2 windows). The
      reject (vs un-run) is proven by firing-witness + survival.
  (D) Content restored — main re-hosts the real viewport (placeholder gone), while
      the outliner slot still shows its placeholder (still floating).
  (E) Read-only — `scene/intervene` on redock_at is rejected (router-driven).
  (F) Accept-after-reject — redock outliner onto MAIN: now it executes too (window
      count -> one, real outliner restored). The guard is a target discriminator
      (only the slot-bearing window hosts), not a per-panel block.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_PANELS = (_OUTLINER, _VIEWPORT, _PROPERTIES)
_FLOATING_PREFIX = "torn-"
_MAIN = "main"
_MAIN_W = 1200
_MAIN_H = 800

# Per-panel content-body tag — present in the MAIN scene only while the panel is
# docked (the placeholder replaces the body when it floats).
_CONTENT = {
    _OUTLINER: "outliner_content_body",
    _VIEWPORT: "viewport_content_body",
    _PROPERTIES: "properties_content_body",
}


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


def _docked(scene: Any, panel: str) -> bool:
    return _scene_contains_tag(scene, _CONTENT[panel])


def _redock_at(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/redock_at")


def _detached(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/detached")


def _tear_off(tf: RpcSubprocess, panel: str) -> None:
    """Float `panel` via the toggle invoke (deterministic — no drag geometry;
    the floater opens at the per-panel cascade position)."""
    tf.invoke(f"/{panel}/external/tear_off", None)


def _redock_into(tf: RpcSubprocess, panel: str, window: str, *, target: str) -> None:
    """AI-primary cross-window redock driver: redock `panel` into `window`'s dock
    zone `target`."""
    tf.invoke(
        f"/{panel}/external/tear_off_redock_at",
        {"window": window, "target": target, "x_rel": 0.5, "y_rel": 0.2},
    )


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — one window, demo panels docked, no redock yet ──
        _section("A: boot — single 1200x800 main window, panels docked")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 boot declares one window; got {boot!r}"
        assert boot[0].get("id") == _MAIN, f"A.2 the boot window is 'main'; got {boot[0]!r}"
        assert boot[0].get("position") is None, f"A.3 main is WM-placed (null position); {boot[0]!r}"
        size = boot[0].get("declared_size")
        assert size == [_MAIN_W, _MAIN_H], f"A.4 main declares its fixed 1200x800 size; got {size!r}"
        scene = _main_scene(tf)
        for panel in _PANELS:
            assert _docked(scene, panel), f"A.5 {panel} is docked (its content body is present)"
            assert not _scene_contains_tag(scene, _placeholder(panel)), (
                f"A.6 {panel} shows no placeholder while docked"
            )
            assert _redock_at(tf, panel) is None, f"A.7 {panel} redock_at null before any redock"
            assert _detached(tf, panel) is False, f"A.8 {panel} not detached at boot"

        # ── (B) tear off outliner + viewport → three windows ─────────
        _section("B: tear off outliner + viewport (two floaters)")
        _tear_off(tf, _OUTLINER)
        _tear_off(tf, _VIEWPORT)
        wait_until(
            lambda: _torn(_OUTLINER) in _window_ids(tf) and _torn(_VIEWPORT) in _window_ids(tf),
            desc="B.1 both floating windows appear",
        )
        ids = _window_ids(tf)
        assert len(ids) == 3, f"B.2 three declared windows after two tear-offs; got {ids!r}"
        assert {_MAIN, _torn(_OUTLINER), _torn(_VIEWPORT)} == ids, f"B.3 the expected windows ({ids!r})"
        # The floating windows opened at their declared cascade positions.
        torn_specs = {w["id"]: w for w in _windows(tf)}
        for panel in (_OUTLINER, _VIEWPORT):
            pos = torn_specs[_torn(panel)].get("position")
            assert pos is not None, f"B.4 {panel}'s floater opens at a declared position; got {pos!r}"
        assert (
            torn_specs[_torn(_OUTLINER)]["position"] != torn_specs[_torn(_VIEWPORT)]["position"]
        ), "B.5 the two floaters cascade to distinct positions"
        scene_b = _main_scene(tf)
        for panel in (_OUTLINER, _VIEWPORT):
            assert _scene_contains_tag(scene_b, _placeholder(panel)), (
                f"B.6 main shows {panel}'s placeholder while floating"
            )
            assert not _docked(scene_b, panel), f"B.7 main no longer paints {panel}'s content body"
        assert _docked(scene_b, _PROPERTIES), "B.8 the un-torn properties panel stays docked"

        # ── (C) mixed invoke — non-main rejected, main executed ──────
        _section("C: redock outliner→non-main (rejected) + viewport→main (executed)")
        _redock_into(tf, _OUTLINER, _torn(_VIEWPORT), target=_OUTLINER)
        _redock_into(tf, _VIEWPORT, _MAIN, target=_PROPERTIES)
        wait_until(lambda: len(_windows(tf)) == 2, desc="C.1 the window count drops to two")
        settled = _window_ids(tf)
        assert _torn(_VIEWPORT) not in settled, "C.2 viewport redocked into main (its floater is gone)"
        assert _torn(_OUTLINER) in settled, (
            "C.3 the outliner floater SURVIVES — its non-main redock target was rejected, "
            "not executed (same firing pass as viewport's execution)"
        )
        vp_at = _redock_at(tf, _VIEWPORT)
        assert isinstance(vp_at, dict), f"C.4 viewport's redock_at recorded the firing ({vp_at})"
        assert vp_at["window"] == _MAIN, f"C.5 viewport's redock targeted main ({vp_at})"
        assert vp_at["panel"] == _VIEWPORT, f"C.6 the payload names the viewport panel ({vp_at})"
        assert vp_at["target"] == _PROPERTIES, f"C.7 the recorded dock zone is the properties pane ({vp_at})"
        out_at = _redock_at(tf, _OUTLINER)
        assert isinstance(out_at, dict), f"C.8 outliner's redock_at ALSO fired ({out_at})"
        assert out_at["window"] == _torn(_VIEWPORT), (
            f"C.9 outliner fired the non-main target it was rejected for ({out_at})"
        )
        assert _detached(tf, _VIEWPORT) is False, "C.10 the executed redock cleared viewport's latch"

        # ── (D) content restored for viewport; outliner still floats ──
        _section("D: main re-hosts viewport; outliner slot still a placeholder")
        scene_d = _main_scene(tf)
        assert _docked(scene_d, _VIEWPORT), "D.1 the real viewport panel is back in main"
        assert not _scene_contains_tag(scene_d, _placeholder(_VIEWPORT)), (
            "D.2 viewport's placeholder is gone (it re-docked)"
        )
        assert _scene_contains_tag(scene_d, _placeholder(_OUTLINER)), (
            "D.3 outliner still shows its placeholder (the non-main redock did not move it)"
        )
        assert not _docked(scene_d, _OUTLINER), "D.4 outliner's real content is still floating"

        # ── (E) redock_at is read-only ───────────────────────────────
        _section("E: redock_at is a read-only diagnostic")
        try:
            tf.request("scene/intervene", {"path": f"/{_VIEWPORT}/external/redock_at", "value": None})
            raise AssertionError("E.1 intervening on redock_at must error")
        except RpcError as exc:
            assert exc.code != 0, f"E.1 redock_at intervene rejected (code {exc.code})"

        # ── (F) accept-after-reject — outliner→main now executes ─────
        _section("F: redock outliner→main — the same panel now executes")
        _redock_into(tf, _OUTLINER, _MAIN, target=_PROPERTIES)
        wait_until(lambda: len(_windows(tf)) == 1, desc="F.1 outliner redocks into main → one window")
        final = _window_ids(tf)
        assert final == {_MAIN}, f"F.2 only main remains ({final})"
        scene_f = _main_scene(tf)
        assert _docked(scene_f, _OUTLINER), "F.3 the real outliner content is back in main"
        assert not _scene_contains_tag(scene_f, _placeholder(_OUTLINER)), (
            "F.4 outliner's placeholder is gone"
        )
        out_final = _redock_at(tf, _OUTLINER)
        assert isinstance(out_final, dict) and out_final["window"] == _MAIN, (
            f"F.5 outliner's last redock targeted main and executed ({out_final})"
        )
        # All five panels' content bodies are now present (full dock restored).
        assert _docked(scene_f, _PROPERTIES) and _docked(scene_f, _VIEWPORT), (
            "F.6 the rest of the dock is intact"
        )

        print("[demo] r1105_editor_tear_off: all sections PASS (editor float-to-window, 2nd consumer)")


if __name__ == "__main__":
    sys.exit(run_demo("r1105_editor_tear_off", body))
