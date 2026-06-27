#!/usr/bin/env python3
"""R1115 §5.16 §5.51 §2 #7 PR-38 — borderless tear-off windows (declared OS chrome).

A torn-off dock panel should look like a floating panel — its own pinion-drawn
header — NOT an OS-titled window that stacks a redundant title bar over that
header. R1115 adds `WindowSpec::decorations` (default `true` = OS-decorated,
byte-identical for every pre-R1115 binding; `false` opts out so the binding owns
the chrome) and surfaces it over `scene/windows` as scene-as-data (§2 #7), so an
AI can read whether each declared window is borderless, not merely where it sits.

`hello-dock-panels-editor` is the forcing consumer: its main window stays
decorated (`decorations:true`) while every torn-off panel floats into a
BORDERLESS window (`decorations:false`) via the single `floating_window_spec`
construction site (the editor owns the panel's chrome — the Blender/Unreal
custom-chrome floating panel a self-hosted editor wants).

The literal "is there an OS title-bar pixel" is HW-gated (needs a real-GPU
windowed host); the DECLARED chrome — the binding's intent the shell drives the
OS window toward — is fully observable headlessly here.

Section roadmap (>=30 assertions across A-F):
  (A) Boot — one decorated main window; `decorations` is a present bool, never null.
  (B) Tear off each panel — every torn-off window is borderless; main stays decorated.
  (C) Full set — exactly one decorated window (main); every torn-off window borderless.
  (D) Redock one — its window drops; main + the survivors keep their declared chrome.
  (E) Redock all — back to the single decorated main (chrome survives the cycle).
  (F) Invariant — `decorations` stayed a present bool on every window all run long.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo, wait_until  # noqa: E402

_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_PANELS = (_OUTLINER, _VIEWPORT, _PROPERTIES)
_FLOATING_PREFIX = "torn-"
_MAIN = "main"
_MAIN_W = 1200
_MAIN_H = 800


# ─── helpers ─────────────────────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    ws = resp.result.get("windows")
    assert isinstance(ws, list), f"scene/windows.windows must be a list; got {resp.result!r}"
    return ws


def _by_id(tf: RpcSubprocess) -> dict[str, dict]:
    return {w["id"]: w for w in _windows(tf)}


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in _windows(tf)}


def _torn(panel: str) -> str:
    return f"{_FLOATING_PREFIX}{panel}"


def _decorations(w: dict) -> bool:
    """Read one window's declared chrome, enforcing the field invariant: it is
    ALWAYS present and ALWAYS a bool (unlike `position`/`declared_size`, which are
    nullable when system-determined — a window's chrome is always declared)."""
    assert "decorations" in w, f"every scene/windows entry must carry decorations: {w!r}"
    d = w["decorations"]
    assert isinstance(d, bool), f"decorations must be a bool, never null: {w!r}"
    return d


def _tear_off(tf: RpcSubprocess, panel: str) -> None:
    """Toggle `panel` between docked and floating via the AI-primary invoke —
    float when docked, dock-back when floating (`toggle_panel_floating`)."""
    tf.invoke(f"/{panel}/external/tear_off", None)


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — one decorated main window ─────────────────────
        _section("A: boot — single main window, OS-decorated by default")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 boot declares one window; got {boot!r}"
        main = boot[0]
        assert main["id"] == _MAIN, f"A.2 the boot window is 'main'; got {main!r}"
        assert _decorations(main) is True, "A.3 main is OS-decorated (the default chrome)"
        assert main.get("position") is None, f"A.4 main is WM-placed (null position); {main!r}"
        assert main.get("declared_size") == [_MAIN_W, _MAIN_H], (
            f"A.5 main declares its fixed size; got {main.get('declared_size')!r}"
        )

        # ── (B) tear off each panel → borderless floaters ────────────
        _section("B: tear off each panel — every torn-off window is borderless")
        for panel in _PANELS:
            _tear_off(tf, panel)
            wait_until(
                lambda p=panel: _torn(p) in _window_ids(tf),
                desc=f"B.1 {panel}'s floating window appears",
            )
            specs = _by_id(tf)
            torn = specs[_torn(panel)]
            assert _decorations(torn) is False, (
                f"B.2 {panel}'s torn-off window is BORDERLESS (decorations:false)"
            )
            assert torn.get("position") is not None, (
                f"B.3 {panel}'s floater still opens at a declared position; got {torn!r}"
            )
            assert _decorations(specs[_MAIN]) is True, (
                f"B.4 main stays decorated while {panel} floats borderless"
            )

        # ── (C) full set — exactly one decorated window (main) ───────
        _section("C: all three torn off — one decorated window, three borderless")
        specs = _by_id(tf)
        assert len(specs) == 4, f"C.1 four declared windows (main + 3 floaters); got {set(specs)!r}"
        decorated = [w for w in specs.values() if _decorations(w)]
        borderless = [w for w in specs.values() if not _decorations(w)]
        assert len(decorated) == 1, f"C.2 exactly one decorated window; got {[w['id'] for w in decorated]!r}"
        assert decorated[0]["id"] == _MAIN, f"C.3 the decorated window is main; got {decorated[0]!r}"
        assert len(borderless) == 3, f"C.4 the three torn-off windows are borderless; got {len(borderless)}"
        assert all(w["id"].startswith(_FLOATING_PREFIX) for w in borderless), (
            f"C.5 every borderless window is a torn-off panel; got {[w['id'] for w in borderless]!r}"
        )

        # ── (D) redock one — its window drops, chrome of the rest holds ─
        _section("D: redock viewport — its window drops; the rest keep their chrome")
        _tear_off(tf, _VIEWPORT)  # toggle → dock-back
        wait_until(
            lambda: _torn(_VIEWPORT) not in _window_ids(tf),
            desc="D.1 the viewport floater drops on redock",
        )
        specs = _by_id(tf)
        assert len(specs) == 3, f"D.2 three windows remain after one redock; got {set(specs)!r}"
        assert _torn(_VIEWPORT) not in specs, "D.3 the viewport's borderless window is gone"
        assert _decorations(specs[_MAIN]) is True, "D.4 main is still decorated after the redock"
        for panel in (_OUTLINER, _PROPERTIES):
            assert _decorations(specs[_torn(panel)]) is False, (
                f"D.5 the still-floating {panel} stays borderless"
            )

        # ── (E) redock the rest — back to the single decorated main ──
        _section("E: redock the rest — back to one decorated main window")
        _tear_off(tf, _OUTLINER)
        _tear_off(tf, _PROPERTIES)
        wait_until(lambda: _window_ids(tf) == {_MAIN}, desc="E.1 only main remains")
        specs = _by_id(tf)
        assert len(specs) == 1, f"E.2 a single window remains; got {set(specs)!r}"
        assert _decorations(specs[_MAIN]) is True, (
            "E.3 main's decorated chrome SURVIVED the full tear/redock cycle (create-time intent)"
        )

        # ── (F) invariant — decorations was a present bool all run long ─
        _section("F: the decorations field stayed a present bool on every window")
        # Re-float once more and confirm the borderless declaration is reproducible
        # (not a one-shot boot artifact) and the field never went null/absent.
        _tear_off(tf, _PROPERTIES)
        wait_until(lambda: _torn(_PROPERTIES) in _window_ids(tf), desc="F.1 properties re-floats")
        final = _by_id(tf)
        assert _decorations(final[_MAIN]) is True, "F.2 main remains decorated"
        assert _decorations(final[_torn(_PROPERTIES)]) is False, (
            "F.3 the re-floated panel is borderless again (reproducible declaration)"
        )
        # `_decorations` asserted bool-ness on every window touched above; restate
        # the invariant explicitly for the final topology.
        assert all(isinstance(w.get("decorations"), bool) for w in final.values()), (
            "F.4 every window reports decorations as a bool, never null"
        )

        print("[demo] r1115_window_decorations: all sections PASS (borderless tear-off chrome, §2 #7)")


if __name__ == "__main__":
    sys.exit(run_demo("r1115_window_decorations", body))
