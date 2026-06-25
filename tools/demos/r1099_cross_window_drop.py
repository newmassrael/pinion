#!/usr/bin/env python3
"""R1099 §5.51 §2 #7 PR-33 — cross-window drop resolution as scene-as-data.

The per-window `InputRouter::resolve_drop_point` sees only its own window's
painted scene, so a drag captured by one window (a settled floating panel)
cannot resolve a dock zone in ANOTHER window (the main dock) — the gap PR-33
closes. R1098 built the geometry substrate (`resolve_cross_window_drop`); R1099
makes it AI-observable through `scene/cross_window_drop`: an agent asks "if a
drop releases at absolute desktop `(x, y)`, which window's dock zone does it
land on?" and gets `{window, tag, x_rel, y_rel}` (or `null` over empty space).

The shell pre-resolves in place (Scene is not Clone — it borrows every window's
stored paint scene `&self` before the dispatch borrow split) and threads the
small owned result to the handler. This is the READ that the live drag→redock
intent (the next slice) will carry; the live winit cross-window grab is
HW-gated, but this resolution is driveable + observable headlessly.

Drives `hello-dock-panels`: a tear-off creates a real second window at a
declared desktop position, then `scene/cross_window_drop` resolves an abs cursor
into either window — proving an abs cursor that maps OUTSIDE the source window
resolves the OTHER window's dock zone (the redock gap), without any cross-window
pointer grab.

Section roadmap (>=30 assertions across A-F):

  (A) Baseline — over the main dock, the resolver names the main window + the
      panel under the cursor (main is WM-placed at the desktop origin, so abs ==
      main-local).
  (B) Tear-off — drag a panel out; a second declared window appears.
  (C) Cross-window hit (the headline) — an abs cursor over the FLOATING window's
      region resolves THAT window's panel, not main: the per-window router could
      never see this, the cross-window resolver does.
  (D) Main still resolves — an abs cursor back over the main dock resolves main,
      independent of the floating window.
  (E) Empty space — an abs cursor over no window's drop target resolves `null`
      (a release there floats, it does not redock).
  (F) Malformed — a request with no cursor is a precise error.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_INSPECTOR_PANEL = "inspector"
_PROPERTY_PANEL = "property"
_VIEWPORT_PANEL = "viewport"
_INSPECTOR_HEADER = "inspector#header"
_FLOATING_PREFIX = "torn-"
_MAIN_W = 880
_MAIN_H = 600


# ─── helpers ─────────────────────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    ws = resp.result.get("windows")
    assert isinstance(ws, list), f"scene/windows.windows must be a list; got {resp.result!r}"
    return ws


def _window_by_id(tf: RpcSubprocess, wid: str) -> Optional[dict]:
    return next((w for w in _windows(tf) if w.get("id") == wid), None)


def _layout(tf: RpcSubprocess) -> Any:
    resp = tf.request("scene/layout", {"viewport": {"width": _MAIN_W, "height": _MAIN_H}})
    assert resp is not None
    return resp.result


def _find_rect(layout: Any, tag: str) -> Optional[dict[str, float]]:
    def walk(node: Any) -> Optional[dict[str, Any]]:
        if not isinstance(node, dict):
            return None
        if node.get("tag") == tag:
            return node.get("rect")
        for child in node.get("children") or []:
            r = walk(child)
            if r is not None:
                return r
        content = node.get("content")
        if isinstance(content, dict):
            return walk(content)
        return None

    rect = walk(layout)
    if not isinstance(rect, dict):
        return None
    return {k: float(rect.get(k, 0)) for k in ("x", "y", "w", "h")}


def _center(rect: dict[str, float]) -> tuple[float, float]:
    return rect["x"] + rect["w"] * 0.5, rect["y"] + rect["h"] * 0.5


def _drag_out(tf: RpcSubprocess, header_tag: str, to_at: tuple[float, float]) -> None:
    tf.request(
        "scene/drag",
        {
            "window": "main",
            "from_path": header_tag,
            "to": {"x": float(to_at[0]), "y": float(to_at[1])},
            "steps": 6,
        },
    )


def _resolve(tf: RpcSubprocess, x: float, y: float) -> Any:
    """`scene/cross_window_drop` — resolve an abs desktop cursor → drop / null."""
    resp = tf.request("scene/cross_window_drop", {"x": float(x), "y": float(y)})
    assert resp is not None and resp.result is not None, "cross_window_drop must answer"
    return resp.result.get("drop")


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    insp_to = (1040.0, 200.0)  # well past the 880px main width → a clean escape

    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) baseline: the main dock resolves to "main" ───────────
        _section("A: an abs cursor over the main dock resolves main")
        prop = _find_rect(_layout(tf), _PROPERTY_PANEL)
        assert prop is not None, "A.1 the property panel has a rect in main's layout"
        px, py = _center(prop)
        drop = _resolve(tf, px, py)
        assert drop is not None, "A.2 a cursor over a main dock zone resolves a drop"
        assert drop["window"] == "main", f"A.3 main is WM-placed at origin → window=main ({drop})"
        assert drop["tag"] == _PROPERTY_PANEL, f"A.4 the resolved tag is the property panel ({drop})"
        assert 0.0 <= drop["x_rel"] <= 1.0 and 0.0 <= drop["y_rel"] <= 1.0, (
            f"A.5 x_rel/y_rel normalised over the zone rect ({drop})"
        )
        assert abs(drop["x_rel"] - 0.5) < 0.2 and abs(drop["y_rel"] - 0.5) < 0.2, (
            f"A.6 a centre cursor normalises near the rect centre ({drop})"
        )

        # ── (B) tear off a panel → a second declared window ──────────
        _section("B: tear-off creates a second declared window")
        assert len(_windows(tf)) == 1, "B.1 one window before the tear-off"
        _drag_out(tf, _INSPECTOR_HEADER, insp_to)
        torn = f"{_FLOATING_PREFIX}{_INSPECTOR_PANEL}"
        entry = wait_until(lambda: _window_by_id(tf, torn), desc="B.2 the floating window appears")
        assert len(_windows(tf)) == 2, "B.3 two declared windows after the tear-off"
        pos = entry.get("position")
        assert isinstance(pos, list) and len(pos) == 2, f"B.4 the floater has a declared position ({entry})"
        fx, fy = float(pos[0]), float(pos[1])
        assert (int(fx), int(fy)) == (int(insp_to[0]), int(insp_to[1])), (
            f"B.5 the floater sits at the escape cursor (main at origin) ({pos})"
        )
        assert isinstance(entry.get("declared_size"), list), (
            f"B.6 the floater carries a declared_size ({entry})"
        )

        # ── (C) cross-window hit — the headline ──────────────────────
        _section("C: an abs cursor over the FLOATING window resolves THAT window")
        # Prime the floating window's paint scene (R684 first-paint finalize on a
        # never-painted window in headless RPC) so its drop targets resolve. The
        # declared_size (R1092) gives the window's logical extent; a content-
        # intrinsic window (null) gets a generous fallback viewport.
        size = entry.get("declared_size")
        fw, fh = (int(size[0]), int(size[1])) if isinstance(size, list) else (320, 480)
        tf.snapshot(source="paint", viewport=(fw, fh), window=torn)
        # An abs cursor a little inside the floating window's region. Its panel
        # root (a drop target) fills the window from its declared origin, so an
        # offset past any header lands inside it.
        cx, cy = fx + 40.0, fy + 80.0
        cross = wait_until(
            lambda: _resolve(tf, cx, cy),
            desc="C.1 the floating window's panel resolves once primed",
        )
        assert cross["window"] == torn, (
            f"C.2 an abs cursor over the floater resolves the FLOATING window, not main ({cross})"
        )
        assert cross["tag"] == _INSPECTOR_PANEL, (
            f"C.3 the resolved tag is the torn inspector panel ({cross})"
        )
        # The per-window router for `main` would resolve None here (the cursor is
        # outside main); the cross-window resolver maps it into the floater. That
        # IS the redock gap PR-33 closes.
        assert cross["window"] != "main", "C.4 main does not own this abs cursor"
        assert 0.0 <= cross["x_rel"] <= 1.0 and 0.0 <= cross["y_rel"] <= 1.0, (
            f"C.5 the floater-local point is normalised ({cross})"
        )
        # The cursor sat a small offset (40, 80) into the floater, so it
        # normalises into the upper-left quadrant of the panel rect.
        assert cross["x_rel"] < 0.5 and cross["y_rel"] < 0.5, (
            f"C.6 the offset maps into the floater's upper-left quadrant ({cross})"
        )

        # ── (D) main still resolves independently ────────────────────
        _section("D: the main dock still resolves after the tear-off")
        prop = _find_rect(_layout(tf), _PROPERTY_PANEL)
        assert prop is not None, "D.1 the property panel still has a main rect"
        px, py = _center(prop)
        drop = _resolve(tf, px, py)
        assert drop is not None and drop["window"] == "main", (
            f"D.2 a cursor over the main dock still resolves main ({drop})"
        )
        assert drop["tag"] == _PROPERTY_PANEL, f"D.3 still the property panel ({drop})"
        # A different main panel (the viewport) also resolves to main — the
        # main window's whole dock is addressable, not just one panel.
        vp = _find_rect(_layout(tf), _VIEWPORT_PANEL)
        if vp is not None:
            vx, vy = _center(vp)
            vdrop = _resolve(tf, vx, vy)
            assert vdrop is not None and vdrop["window"] == "main", (
                f"D.4 the viewport panel also resolves to main ({vdrop})"
            )
            assert vdrop["tag"] == _VIEWPORT_PANEL, f"D.5 resolves the viewport tag ({vdrop})"

        # ── (E) empty space → null (a release there floats) ──────────
        _section("E: an abs cursor over no drop target resolves null")
        assert _resolve(tf, 5000.0, 5000.0) is None, "E.1 far empty space resolves null"
        # The gap between the two windows (past main's right edge, before the
        # floater) is also empty.
        assert _resolve(tf, float(_MAIN_W + 20), 50.0) is None, "E.2 the inter-window gap is null"
        # Below both windows is also empty (main ends at MAIN_H, the floater
        # is offset down but bounded).
        assert _resolve(tf, 400.0, float(_MAIN_H + 1000)) is None, "E.3 below the dock is null"

        # ── (F) malformed request → precise error ────────────────────
        _section("F: a request with no cursor is a precise error")
        try:
            tf.request("scene/cross_window_drop", {})
            raise AssertionError("F.1 a missing cursor must error")
        except RpcError as exc:
            assert exc.code != 0, f"F.1 missing cursor rejected (code {exc.code})"
        try:
            tf.request("scene/cross_window_drop", {"x": 1.0})
            raise AssertionError("F.2 a half-cursor must error")
        except RpcError as exc:
            assert exc.code != 0, f"F.2 incomplete cursor rejected (code {exc.code})"

        print("[demo] r1099_cross_window_drop: all sections PASS")


if __name__ == "__main__":
    sys.exit(run_demo("r1099_cross_window_drop", body))
