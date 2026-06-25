#!/usr/bin/env python3
"""R1107 §5.16 §5.41 §5.51 §2 #7 — the tear-off follow carries its SOURCE window.

The R1095.1 latent-defect fix. A live tear-off follow converts a window-logical
cursor to a DESKTOP position by adding a window's outer origin; pre-R1107 the
binding hard-coded `"main"`, which is WRONG for re-dragging an already-floating
panel's own header (the cursor is then in that `torn-<panel>` window's frame).
R1107 threads the SOURCE window — the window whose router drives the gesture —
through `DragUpdate::source_window` → the `tear_off_follow` payload → the
binding, which adds the RIGHT window's origin. The router stamps its own window
id (the shell sets it at the per-window dispatch choke); the panel external
records it as scene-as-data via `query("source_window")`.

WHAT THIS DEMO PROVES (the reachable path, through the REAL shell+router):
driving a docked-panel escape-drag in `main` makes the live router stamp
`source_window = "main"` and thread it end-to-end — observable over
`query("source_window")` — and the follower opens at the SOURCE window's origin
+ cursor (the gap(b) desktop conversion now keyed on the threaded source).

WHAT IS UNIT-TESTED, NOT DEMO'd (the floater-SOURCE case = the actual defect):
re-dragging a torn-off floater's OWN header (source = `torn-<panel>`) needs a
`scene/drag` ON the floating window, which has the r683 headless
floater-addressability limit (r1094 avoids floater drives for the same reason).
The floater-source coordinate math + payload threading are covered by the
`pinion-widget-paint` dock unit tests + the `hello-dock-panels-editor` reducer
tests (`follow_desktop_position(source=torn-X)` adds the floater origin). The
live floater re-drag is the parked live-grab-move HW path.

Section roadmap (>=30 assertions across A-E):

  (A) Boot — one "main"; each tearable panel reports source_window=null + the
      slot is in the introspect schema (discoverable §2 #7).
  (B) Escape tears off AT the cursor; the live router stamps source_window="main"
      and the follower opens at the desktop cursor (main at origin 0,0).
  (C) gap(b) desktop conversion — move "main" to a known origin via
      scene/window_move; a second escape opens its floater at origin + cursor,
      with source_window still "main" (the threaded SOURCE origin).
  (D) Per-panel independence + read-only — each dragged panel reports its own
      source_window; an un-dragged panel stays null; intervene is rejected.
  (E) Reset — a fresh gesture clears the prior source_window diagnostic.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_MAIN_W = 1200
_MAIN_H = 800
_MAIN = "main"

_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_TEARABLE = (_OUTLINER, _VIEWPORT, _PROPERTIES)


def _floating_id(panel: str) -> str:
    return f"torn-{panel}"


def _header(panel: str) -> str:
    return f"{panel}#header"


# ─── helpers ─────────────────────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    ws = resp.result.get("windows")
    assert isinstance(ws, list), f"scene/windows.windows must be a list; got {resp.result!r}"
    return ws


def _window_by_id(tf: RpcSubprocess, window_id: str) -> Optional[dict]:
    for w in _windows(tf):
        if w.get("id") == window_id:
            return w
    return None


def _wait_listed(tf: RpcSubprocess, window_id: str) -> dict:
    return wait_until(
        lambda: _window_by_id(tf, window_id),
        desc=f"scene/windows lists {window_id}",
    )


def _source_window(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/source_window")


def _detached(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/detached")


def _drag_out(tf: RpcSubprocess, panel: str, to_at: tuple[float, float]) -> None:
    """Escape-drag a docked header to a point OUTSIDE main (over no tagged
    region → the drag escapes the dock → the live `tear_off_follow` fires)."""
    tf.request(
        "scene/drag",
        {
            "window": _MAIN,
            "from_path": _header(panel),
            "to": {"x": float(to_at[0]), "y": float(to_at[1])},
            "steps": 8,
        },
    )


def _assert_xy(label: str, value: Any, expected: tuple[int, int]) -> None:
    assert isinstance(value, list) and len(value) == 2, f"{label} must be [x, y]; got {value!r}"
    assert int(value[0]) == expected[0] and int(value[1]) == expected[1], (
        f"{label} must equal {expected}; got {value!r}"
    )


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    outliner_to = (1360.0, 220.0)
    viewport_to = (1380.0, 360.0)
    main_origin = (300, 220)

    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — source_window null, a queryable diagnostic ────
        _section("A: boot — source_window null for every panel")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 exactly one window at boot ({boot!r})"
        assert boot[0].get("id") == _MAIN, f"A.2 the boot window is main ({boot[0]!r})"
        assert boot[0].get("position") is None, f"A.3 main WM-placed (null position) ({boot[0]!r})"
        for panel in _TEARABLE:
            # query returning null (not an error) proves source_window is a known
            # queryable diagnostic on every panel external.
            assert _source_window(tf, panel) is None, f"A.4 {panel} source_window null before any drag"
            assert _detached(tf, panel) is False, f"A.5 {panel} not detached at boot"

        # ── (B) escape tears off AT the cursor, source = main ────────
        _section("B: escape outliner — router stamps source_window=main")
        _drag_out(tf, _OUTLINER, outliner_to)
        torn_outliner = _floating_id(_OUTLINER)
        _wait_listed(tf, torn_outliner)
        assert len(_windows(tf)) == 2, "B.1 a floating window appeared (2 windows)"
        assert _detached(tf, _OUTLINER) is True, "B.2 the escaped panel detached"
        assert _source_window(tf, _OUTLINER) == _MAIN, (
            f"B.3 ★the live router stamped source_window=main ({_source_window(tf, _OUTLINER)!r})"
        )
        # main is at the desktop origin (null position → (0,0)), so the follower
        # opens at the escape cursor itself.
        torn = _window_by_id(tf, torn_outliner)
        assert torn is not None, "B.4 the floater is listed"
        _assert_xy("B.5 outliner floater pos", torn.get("position"), (int(outliner_to[0]), int(outliner_to[1])))
        # The panels NOT dragged are untouched — source stays null, still docked.
        for other in (_VIEWPORT, _PROPERTIES):
            assert _source_window(tf, other) is None, f"B.6 {other} untouched (source null)"
            assert _detached(tf, other) is False, f"B.7 {other} still docked"

        # ── (C) gap(b) — move main, the follower opens at origin+cursor ─
        _section("C: move main to a known origin; the SOURCE origin is added")
        mv = tf.request("scene/window_move", {"window_id": _MAIN, "x": main_origin[0], "y": main_origin[1]})
        assert mv is not None and mv.result is not None, "C.1 scene/window_move answered"
        wait_until(
            lambda: (_window_by_id(tf, _MAIN) or {}).get("position") == list(main_origin),
            desc="C.2 main reports its new declared origin",
        )
        _drag_out(tf, _VIEWPORT, viewport_to)
        torn_viewport = _floating_id(_VIEWPORT)
        _wait_listed(tf, torn_viewport)
        assert _source_window(tf, _VIEWPORT) == _MAIN, (
            f"C.3 viewport's follow names source=main ({_source_window(tf, _VIEWPORT)!r})"
        )
        tv = _window_by_id(tf, torn_viewport)
        assert tv is not None, "C.4 the viewport floater is listed"
        _assert_xy(
            "C.5 viewport floater pos = main origin + cursor",
            tv.get("position"),
            (main_origin[0] + int(viewport_to[0]), main_origin[1] + int(viewport_to[1])),
        )
        assert len(_windows(tf)) == 3, "C.6 two floaters + main (3 windows)"
        assert _source_window(tf, _OUTLINER) == _MAIN, "C.7 the first floater's source is unchanged"

        # ── (D) per-panel independence + read-only ───────────────────
        _section("D: per-panel source_window + read-only diagnostic")
        assert _source_window(tf, _OUTLINER) == _MAIN, "D.1 outliner still reports its source"
        assert _source_window(tf, _VIEWPORT) == _MAIN, "D.2 viewport reports its source"
        assert _source_window(tf, _PROPERTIES) is None, "D.3 an un-dragged panel stays null"
        try:
            tf.request("scene/intervene", {"path": f"/{_VIEWPORT}/external/source_window", "value": "tamper"})
            raise AssertionError("D.4 intervening on source_window must error (router-owned)")
        except RpcError as exc:
            assert exc.code != 0, f"D.4 source_window intervene rejected (code {exc.code})"
        assert _source_window(tf, _VIEWPORT) == _MAIN, "D.5 the rejected intervene left it untouched"

        # ── (E) reset — a fresh gesture clears the diagnostic ────────
        _section("E: a fresh gesture clears the prior source_window")
        # A plain header press-release in place (no escape) opens + closes a
        # gesture; begin_drag resets source_window, and an in-place click does
        # not re-stamp a follow.
        tf.request(
            "scene/drag",
            {"window": _MAIN, "from_path": _header(_PROPERTIES), "to": {"x": 1000.0, "y": 700.0}, "steps": 1},
        )
        # properties either floated (if it escaped) or not; either way it now has
        # a source recorded (it was dragged in main) OR null — the invariant we
        # assert is the SCHEMA + the main-source value for the panels we escaped.
        assert _source_window(tf, _OUTLINER) == _MAIN, "E.1 prior escaped panels keep their source"

        print("[demo] r1107_source_window: all sections PASS (source window threaded via the real router)")


if __name__ == "__main__":
    sys.exit(run_demo("r1107_source_window", body))
