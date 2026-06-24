#!/usr/bin/env python3
"""R1087 §5.16 §5.41 §2 #7 PR-31 — dock tear-off floating-window POSITION
as scene-as-data.

Drives `hello-dock-panels`, the `windows_signal` consumer, to exercise the
PR-31 foundation: a torn-off panel's floating window carries a DECLARED
logical-pixel position (`WindowSpec::position`), and that position is
observable over the new read-only `scene/windows` RPC — so an AI sees WHERE
each floating panel's window sits, not merely that it exists (the §2 #7
obligation for the new position state). The live OS placement +
drag-to-follow is the next PR-31 slice (HW-gated); this demo verifies the
SSOT (declared position) end-to-end through the binding → `windows_signal`
→ `reconcile_windows` arc, headlessly.

Section roadmap (>=30 assertions across A-F):

  (A) `scene/windows` boot — exactly one declared window ("main"), with a
      WM-placed (null) position; the read is reachable + well-shaped.
  (B) Tear-off places a positioned window — drag the inspector header past
      the threshold; `scene/windows` grows a `torn-inspector` entry whose
      declared position is the binding's per-panel cascade offset.
  (C) Multi-tear-off cascade — tear off property + viewport too; each
      floating window reports a DISTINCT declared position (no stacking).
  (D) Position is per-panel deterministic — the reported positions match
      the binding's `floating_window_position` cascade exactly.
  (E) Dock-back removes the entry — dock the inspector back; `scene/windows`
      drops `torn-inspector` and its position with it.
  (F) Return to baseline — dock the rest back; `scene/windows` is once more
      just the single WM-placed "main".
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

# ─── constants mirrored from the binding ────────────────────────────

_MAIN_W = 880
_MAIN_H = 600

_INSPECTOR_PANEL_TAG = "inspector"
_PROPERTY_PANEL_TAG = "property"
_VIEWPORT_PANEL_TAG = "viewport"

_FLOATING_PREFIX = "torn-"


def _floating_id(panel_id: str) -> str:
    return f"{_FLOATING_PREFIX}{panel_id}"


# `floating_window_position` in the binding: base (120, 90) + step*40, with
# step = inspector 0 / property 1 / viewport 2.
def _expected_position(panel_id: str) -> tuple[int, int]:
    step = {
        _INSPECTOR_PANEL_TAG: 0,
        _PROPERTY_PANEL_TAG: 1,
        _VIEWPORT_PANEL_TAG: 2,
    }[panel_id]
    return (120 + step * 40, 90 + step * 40)


# ─── scene/windows helpers ──────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    """The declared-window list from `scene/windows` (a global read; no
    `{window}` scope needed)."""
    resp = tf.request("scene/windows", {})
    assert resp is not None, "scene/windows returned no response"
    result = resp.result
    assert isinstance(result, dict), f"scene/windows result must be an object; got {result!r}"
    windows = result.get("windows")
    assert isinstance(windows, list), f"scene/windows.windows must be a list; got {windows!r}"
    return windows


def _window_by_id(tf: RpcSubprocess, window_id: str) -> Optional[dict]:
    for w in _windows(tf):
        if w.get("id") == window_id:
            return w
    return None


def _position_of(tf: RpcSubprocess, window_id: str) -> Any:
    w = _window_by_id(tf, window_id)
    assert w is not None, f"scene/windows must list {window_id!r}"
    return w.get("position")


def _toggle_float(tf: RpcSubprocess, panel_id: str) -> None:
    """Toggle a panel between docked and floating via the `tear_off`
    intent (the binding's `toggle_panel_floating` reducer). Deterministic,
    gesture-free: invoking it on a docked panel pushes the positioned
    floating `WindowSpec`; invoking it again docks back. R1087 verifies the
    POSITION the reducer writes, independent of which gesture triggers it —
    a drag-to-centre would now hit PR-29's reorganize/Tabify zone, so the
    invoke toggle is the robust tear-off driver (it is also r683's
    dock-back path)."""
    tf.invoke(f"/{panel_id}/external/tear_off", None)


def _wait_listed(tf: RpcSubprocess, window_id: str) -> dict:
    """Window creation is async (reconcile Effect spawns the OS window after
    the tear-off reducer commits). Poll `scene/windows` until the id appears
    (R883 zero-flake)."""
    return wait_until(
        lambda: _window_by_id(tf, window_id),
        desc=f"scene/windows lists {window_id}",
    )


def _wait_unlisted(tf: RpcSubprocess, window_id: str) -> None:
    wait_until(
        lambda: _window_by_id(tf, window_id) is None or None,
        desc=f"scene/windows drops {window_id}",
    )


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) scene/windows boot — single WM-placed "main" ──────
        boot = _windows(tf)
        assert len(boot) == 1, f"boot must declare exactly one window; got {boot!r}"
        main = boot[0]
        assert main.get("id") == "main", f"boot window id must be 'main'; got {main!r}"
        assert isinstance(main.get("title"), str) and main["title"], (
            f"main must carry a non-empty title; got {main!r}"
        )
        assert main.get("position") is None, (
            f"main is WM-placed at boot — position must be null; got {main!r}"
        )

        # ── (B) tear-off places a positioned floating window ──────
        _toggle_float(tf, _INSPECTOR_PANEL_TAG)
        torn_inspector = _floating_id(_INSPECTOR_PANEL_TAG)
        entry = _wait_listed(tf, torn_inspector)
        # Now two declared windows.
        assert len(_windows(tf)) == 2, "tear-off must add exactly one declared window"
        # The floating window's declared position is the binding's offset.
        exp = list(_expected_position(_INSPECTOR_PANEL_TAG))
        assert entry.get("position") == exp, (
            f"{torn_inspector} declared position must be {exp}; got {entry.get('position')!r}"
        )
        # Title is the floating title, distinct from main.
        assert "floating" in (entry.get("title") or ""), (
            f"floating window title must mark it floating; got {entry!r}"
        )
        # main is still WM-placed (null) — the tear-off touched only the new
        # spec, never the primary's placement.
        assert _position_of(tf, "main") is None, "main position must stay null after a tear-off"

        # ── (C) multi-tear-off cascade — distinct positions ───────
        _toggle_float(tf, _PROPERTY_PANEL_TAG)
        _toggle_float(tf, _VIEWPORT_PANEL_TAG)
        _wait_listed(tf, _floating_id(_PROPERTY_PANEL_TAG))
        _wait_listed(tf, _floating_id(_VIEWPORT_PANEL_TAG))
        # 1 main + 3 floating.
        all_windows = _windows(tf)
        assert len(all_windows) == 4, f"cascade must yield 4 declared windows; got {all_windows!r}"
        # Window ids are unique — no duplicate spec leaked into the signal.
        ids = [w.get("id") for w in all_windows]
        assert len(set(ids)) == len(ids), f"declared window ids must be unique; got {ids!r}"
        assert "main" in ids, "main must remain declared through the cascade"
        positions = [
            tuple(_position_of(tf, _floating_id(p)))
            for p in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG)
        ]
        assert len(set(positions)) == 3, (
            f"every floating window must have a DISTINCT position; got {positions!r}"
        )
        # Each floating position is a well-formed 2-int pair on the wire.
        for panel in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            pos = _position_of(tf, _floating_id(panel))
            assert isinstance(pos, list) and len(pos) == 2, (
                f"{_floating_id(panel)} position must be a [x, y] pair; got {pos!r}"
            )
            assert all(isinstance(c, int) for c in pos), (
                f"{_floating_id(panel)} position components must be integers; got {pos!r}"
            )
        # main stays WM-placed (null) even with three floating windows out.
        assert _position_of(tf, "main") is None, "main position stays null through the cascade"

        # ── (D) position is the binding's deterministic cascade ───
        for panel in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            exp = list(_expected_position(panel))
            got = _position_of(tf, _floating_id(panel))
            assert got == exp, (
                f"{_floating_id(panel)} position must be {exp} (binding cascade); got {got!r}"
            )
        # Positions are strictly increasing in both axes (the +40 step).
        xs = [_expected_position(p)[0] for p in
              (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG)]
        ys = [_expected_position(p)[1] for p in
              (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG)]
        assert xs == sorted(xs) and len(set(xs)) == 3, "cascade x offsets must strictly increase"
        assert ys == sorted(ys) and len(set(ys)) == 3, "cascade y offsets must strictly increase"

        # ── (E) dock-back removes the entry + its position ────────
        # Toggle inspector back (the same reducer; floating → docked).
        _toggle_float(tf, _INSPECTOR_PANEL_TAG)
        _wait_unlisted(tf, torn_inspector)
        after = _windows(tf)
        assert all(w.get("id") != torn_inspector for w in after), (
            f"dock-back must drop {torn_inspector} from scene/windows; got {after!r}"
        )
        assert len(after) == 3, f"dock-back must leave 3 declared windows; got {after!r}"

        # ── (F) return to baseline — only WM-placed "main" ────────
        for panel in (_PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            _toggle_float(tf, panel)
            _wait_unlisted(tf, _floating_id(panel))
        final = _windows(tf)
        assert len(final) == 1 and final[0].get("id") == "main", (
            f"final state must be the single 'main' window; got {final!r}"
        )
        assert final[0].get("position") is None, "main returns to WM-placed (null) baseline"


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1087 §5.16 §5.41 §2 #7 PR-31 — dock tear-off floating-window position as scene-as-data",
        body,
    ))
