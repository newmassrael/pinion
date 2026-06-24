#!/usr/bin/env python3
"""R1092 §5.16 §5.41 §2 #7 — declared window SIZE as scene-as-data, completing
the `scene/windows` geometry introspection surface.

R1087 gave an AI the declared POSITION of each declared window; this adds the
other half of the geometry — the declared open SIZE (`declared_size`, `[w, h]`
logical pixels). So an AI introspecting a multi-window / docked editor layout
reads not merely WHERE each (torn-off) window sits but HOW BIG it is declared
to be. Sourced from the binding's `SizeStrategy` SSOT
(`SizeStrategy::declared_size`): a `Fixed`/`OpenResizable` window declares an
exact size; a content-intrinsic `IntrinsicAfterFirstPaint` window declares
`null` (its eventual size is the post-first-paint content bbox) — the same
`null`-means-system-determined honesty `position` uses for a WM-placed window.

Drives `hello-dock-panels` (every window is `Fixed`, so all declare a size)
end-to-end through the binding → `windows_signal` → `reconcile_windows` arc,
headlessly. The `null` (intrinsic) case is pinned by the pinion-shell +
pinion-rpc unit tests; this demo verifies the declared-size SSOT flows to the
wire for a realistic dock binding, and that size + position are independent
axes (the main window declares a size but no position).

Section roadmap (>=30 assertions across A-F):

  (A) `scene/windows` boot — the single "main" window declares its exact
      Fixed size `[880, 600]` while its position is null (WM-placed): the
      two geometry axes are independent.
  (B) Tear-off declares a sized floating window — the torn inspector reports
      its Fixed `[360, 360]` declared size alongside its cascade position.
  (C) Multi-tear-off — every floating window declares the SAME `[360, 360]`
      size (uniform Fixed spec) while positions stay distinct; size + position
      are both well-formed `[_, _]` int pairs.
  (D) Geometry coherence — every declared window carries a present-or-null
      `declared_size`; main keeps its size with a null position throughout.
  (E) Dock-back drops the entry and its declared size with it.
  (F) Return to baseline — once more just the single sized, WM-placed "main".
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo, wait_until  # noqa: E402

# ─── constants mirrored from the binding ────────────────────────────

# `hello-dock-panels` opens main at Fixed 880x600 and every torn-off panel
# at Fixed 360x360 (src/main.rs MAIN_W/MAIN_H, FLOATING_W/FLOATING_H).
_MAIN_SIZE = [880, 600]
_FLOATING_SIZE = [360, 360]

_INSPECTOR_PANEL_TAG = "inspector"
_PROPERTY_PANEL_TAG = "property"
_VIEWPORT_PANEL_TAG = "viewport"

_FLOATING_PREFIX = "torn-"


def _floating_id(panel_id: str) -> str:
    return f"{_FLOATING_PREFIX}{panel_id}"


# ─── scene/windows helpers ──────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    """The declared-window list from `scene/windows` (a global read)."""
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


def _size_of(tf: RpcSubprocess, window_id: str) -> Any:
    w = _window_by_id(tf, window_id)
    assert w is not None, f"scene/windows must list {window_id!r}"
    assert "declared_size" in w, (
        f"{window_id} entry must carry a declared_size key; got {w!r}"
    )
    return w.get("declared_size")


def _position_of(tf: RpcSubprocess, window_id: str) -> Any:
    w = _window_by_id(tf, window_id)
    assert w is not None, f"scene/windows must list {window_id!r}"
    return w.get("position")


def _assert_size_pair(label: str, size: Any, expected: list[int]) -> None:
    assert isinstance(size, list) and len(size) == 2, (
        f"{label} declared_size must be a [w, h] pair; got {size!r}"
    )
    assert all(isinstance(c, int) for c in size), (
        f"{label} declared_size components must be integers; got {size!r}"
    )
    assert all(c > 0 for c in size), (
        f"{label} declared_size must be positive; got {size!r}"
    )
    assert size == expected, f"{label} declared_size must be {expected}; got {size!r}"


def _toggle_float(tf: RpcSubprocess, panel_id: str) -> None:
    """Toggle a panel between docked and floating via the `tear_off` intent
    (the binding's `toggle_panel_floating` reducer) — the robust, gesture-free
    tear-off driver r1087 also uses."""
    tf.invoke(f"/{panel_id}/external/tear_off", None)


def _wait_listed(tf: RpcSubprocess, window_id: str) -> dict:
    """Window creation is async; poll until the id appears (R883 zero-flake)."""
    return wait_until(
        lambda: _window_by_id(tf, window_id),
        desc=f"scene/windows lists {window_id}",
    )


def _wait_unlisted(tf: RpcSubprocess, window_id: str) -> None:
    wait_until(
        lambda: _window_by_id(tf, window_id) is None,
        desc=f"scene/windows drops {window_id}",
    )


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) boot — main declares a size; position is null ─────
        boot = _windows(tf)
        assert len(boot) == 1, f"boot must declare exactly one window; got {boot!r}"
        main = boot[0]
        assert main.get("id") == "main", f"boot window id must be 'main'; got {main!r}"
        _assert_size_pair("main", main.get("declared_size"), _MAIN_SIZE)
        # The two geometry axes are independent: main declares its Fixed size
        # but leaves placement to the WM (position null).
        assert main.get("position") is None, (
            f"main is WM-placed at boot — position must be null even though it "
            f"declares a size; got {main!r}"
        )

        # ── (B) tear-off declares a sized floating window ─────────
        _toggle_float(tf, _INSPECTOR_PANEL_TAG)
        torn_inspector = _floating_id(_INSPECTOR_PANEL_TAG)
        entry = _wait_listed(tf, torn_inspector)
        assert len(_windows(tf)) == 2, "tear-off must add exactly one declared window"
        _assert_size_pair(torn_inspector, entry.get("declared_size"), _FLOATING_SIZE)
        # The floating window carries BOTH a declared size and a position.
        assert entry.get("position") is not None, (
            f"{torn_inspector} is a positioned floating window; position must be "
            f"non-null alongside its declared size; got {entry!r}"
        )
        # main keeps its size + null position — the tear-off touched only the
        # new spec.
        _assert_size_pair("main", _size_of(tf, "main"), _MAIN_SIZE)
        assert _position_of(tf, "main") is None, "main position stays null after a tear-off"

        # ── (C) multi-tear-off — uniform size, distinct positions ─
        _toggle_float(tf, _PROPERTY_PANEL_TAG)
        _toggle_float(tf, _VIEWPORT_PANEL_TAG)
        _wait_listed(tf, _floating_id(_PROPERTY_PANEL_TAG))
        _wait_listed(tf, _floating_id(_VIEWPORT_PANEL_TAG))
        all_windows = _windows(tf)
        assert len(all_windows) == 4, f"cascade must yield 4 declared windows; got {all_windows!r}"
        # Every floating window declares the SAME Fixed [360, 360] size.
        for panel in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            fid = _floating_id(panel)
            _assert_size_pair(fid, _size_of(tf, fid), _FLOATING_SIZE)
        floating_sizes = [
            _size_of(tf, _floating_id(p))
            for p in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG)
        ]
        assert all(s == _FLOATING_SIZE for s in floating_sizes), (
            f"all floating windows share the Fixed declared size; got {floating_sizes!r}"
        )
        # Positions stay DISTINCT even though sizes are identical — size and
        # position are independent geometry axes.
        positions = [
            tuple(_position_of(tf, _floating_id(p)))
            for p in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG)
        ]
        assert len(set(positions)) == 3, (
            f"floating windows must keep distinct positions despite equal sizes; got {positions!r}"
        )

        # ── (D) geometry coherence across the whole declared set ──
        for w in all_windows:
            assert "declared_size" in w, f"every window entry must carry declared_size; got {w!r}"
            ds = w.get("declared_size")
            # Every window in this all-Fixed binding declares a concrete size
            # (none is content-intrinsic), so none is null here.
            assert ds is not None, (
                f"{w.get('id')!r} is a Fixed window — declared_size must be non-null; got {w!r}"
            )
            assert isinstance(ds, list) and len(ds) == 2, (
                f"{w.get('id')!r} declared_size must be a [w, h] pair; got {ds!r}"
            )
        # main alone keeps size-without-position; floating windows have both.
        _assert_size_pair("main", _size_of(tf, "main"), _MAIN_SIZE)
        assert _position_of(tf, "main") is None, "main keeps a null position through the cascade"

        # ── (E) dock-back drops the entry + its declared size ─────
        _toggle_float(tf, _INSPECTOR_PANEL_TAG)
        _wait_unlisted(tf, torn_inspector)
        after = _windows(tf)
        assert all(w.get("id") != torn_inspector for w in after), (
            f"dock-back must drop {torn_inspector} (and its declared size); got {after!r}"
        )
        assert len(after) == 3, f"dock-back must leave 3 declared windows; got {after!r}"

        # ── (F) return to baseline — single sized, WM-placed main ─
        for panel in (_PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            _toggle_float(tf, panel)
            _wait_unlisted(tf, _floating_id(panel))
        final = _windows(tf)
        assert len(final) == 1 and final[0].get("id") == "main", (
            f"final state must be the single 'main' window; got {final!r}"
        )
        _assert_size_pair("main", final[0].get("declared_size"), _MAIN_SIZE)
        assert final[0].get("position") is None, "main returns to WM-placed (null) baseline"


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1092 §5.16 §5.41 §2 #7 — declared window size as scene-as-data",
        body,
    ))
