#!/usr/bin/env python3
"""R1093 §5.15 §5.51 §2 #7 — the live drag cursor as scene-as-data.

The §5.15 input-forwarding ENRICHMENT: the router now hands a drag source the
absolute window-logical cursor on every move + the release (via the additive,
default-delegating `External::drag_to_at` / `drag_release_at`), not merely the
rect-relative DropPoint — which goes `None` the instant the cursor escapes
every tagged region (the tear-off case). `DockPanelExternal` records that
cursor and exposes it as `query("drag_cursor")` (`[x, y]` / null), so an AI
observes WHERE a drag gesture went — the seam a follow-the-cursor tear-off
coordinator (the next slice) reads.

Drives `hello-dock-panels`. Each drag releases the dragged header back INSIDE
the panel's own rect (a self-drop → snap-back, NOT a tear-off), so the panel
external stays addressable in the main window and `drag_cursor` is queryable
post-gesture. The cursor the slot reports must equal the `scene/drag` release
point to the pixel.

Section roadmap (>=30 assertions across A-E):

  (A) Boot — every panel's drag_cursor is null (no drag yet) and the slot is
      in the introspect schema (discoverable).
  (B) A drag forwards the cursor — drag the inspector header to a point inside
      the inspector rect; drag_cursor becomes that exact [x, y], the panel did
      NOT tear off (still docked), and the value is a well-formed float pair.
  (C) Tracks distinct release points — a second drag to a different in-rect
      point updates drag_cursor to the new point (not the stale one).
  (D) Per-panel independence — dragging the PROPERTY panel sets its own
      drag_cursor while the inspector's stays at its last value.
  (E) Read-only — intervene on drag_cursor is rejected (router-driven slot).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_MAIN_W = 880
_MAIN_H = 600

_INSPECTOR_PANEL_TAG = "inspector"
_PROPERTY_PANEL_TAG = "property"
_VIEWPORT_PANEL_TAG = "viewport"
_INSPECTOR_HEADER_TAG = "inspector#header"
_PROPERTY_HEADER_TAG = "property#header"

_TOL = 1.5  # window-logical px: integer drive coords round-trip exact; slack
            # only guards float repr.


# ─── helpers ─────────────────────────────────────────────────────────


def _find_node_by_tag(node: Any, target: str) -> Optional[dict]:
    if not isinstance(node, dict):
        return None
    if node.get("tag") == target:
        return node
    for child in node.get("children") or []:
        r = _find_node_by_tag(child, target)
        if r is not None:
            return r
    content = node.get("content")
    if isinstance(content, dict):
        return _find_node_by_tag(content, target)
    return None


def _panel_rect(tf: RpcSubprocess, tag: str) -> tuple[float, float, float, float]:
    """The post-layout rect (x, y, w, h) of a tagged node in the main window."""
    layout = tf.request("scene/layout", {"viewport": {"width": _MAIN_W, "height": _MAIN_H}})
    assert layout is not None and layout.result is not None, "scene/layout must answer"
    node = _find_node_by_tag(layout.result, tag)
    assert node is not None, f"layout must contain {tag!r}"
    r = node.get("rect") or {}
    return (float(r["x"]), float(r["y"]), float(r["w"]), float(r["h"]))


def _query(tf: RpcSubprocess, path: str) -> Any:
    return tf.query(path)


def _drag_cursor(tf: RpcSubprocess, panel_tag: str) -> Any:
    return _query(tf, f"/{panel_tag}/external/drag_cursor")


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


def _drag_to(tf: RpcSubprocess, header_tag: str, to_at: tuple[float, float]) -> None:
    tf.request(
        "scene/drag",
        {
            "window": "main",
            "from_path": header_tag,
            "to": {"x": float(to_at[0]), "y": float(to_at[1])},
            "steps": 8,
        },
    )


def _assert_in_rect(label: str, point: tuple[float, float], rect: tuple[float, float, float, float]) -> None:
    """A release inside the panel's own rect is a self-drop → snap-back (NOT a
    tear-off), the precondition that keeps the external addressable in main."""
    x, y, w, h = rect
    assert x <= point[0] <= x + w, f"{label} x {point[0]} must be within [{x}, {x + w}]"
    assert y <= point[1] <= y + h, f"{label} y {point[1]} must be within [{y}, {y + h}]"


def _assert_cursor_at(label: str, cursor: Any, expected: tuple[float, float]) -> None:
    assert isinstance(cursor, list) and len(cursor) == 2, (
        f"{label} drag_cursor must be an [x, y] pair; got {cursor!r}"
    )
    assert all(isinstance(c, (int, float)) for c in cursor), (
        f"{label} drag_cursor components must be numbers; got {cursor!r}"
    )
    assert abs(cursor[0] - expected[0]) <= _TOL and abs(cursor[1] - expected[1]) <= _TOL, (
        f"{label} drag_cursor must equal the release point {expected}; got {cursor!r}"
    )


def _wait_cursor(tf: RpcSubprocess, panel_tag: str, expected: tuple[float, float]) -> Any:
    """Drag dispatch + the forwarded cursor settle synchronously, but poll for
    zero-flake robustness against any deferred paint."""
    def ready() -> Any:
        c = _drag_cursor(tf, panel_tag)
        if (
            isinstance(c, list)
            and len(c) == 2
            and abs(c[0] - expected[0]) <= _TOL
            and abs(c[1] - expected[1]) <= _TOL
        ):
            return c
        return None

    return wait_until(ready, desc=f"{panel_tag} drag_cursor reaches {expected}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) boot — drag_cursor null, slot discoverable ────────
        for panel in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            assert _drag_cursor(tf, panel) is None, (
                f"{panel} drag_cursor must be null before any drag"
            )
        # A queryable slot that returns null (not an error) proves the slot
        # exists on the external — an unknown path would raise instead.

        ins = _panel_rect(tf, _INSPECTOR_PANEL_TAG)
        assert ins[2] > 50 and ins[3] > 50, f"inspector rect must be sized; got {ins!r}"

        # ── (B) a drag forwards the cursor ────────────────────────
        # Release INSIDE the inspector rect → self-drop → snap-back (no
        # tear-off), so the external stays in main + drag_cursor is queryable.
        p1 = (round(ins[0] + ins[2] * 0.5), round(ins[1] + ins[3] * 0.55))
        _assert_in_rect("p1", p1, ins)
        _drag_to(tf, _INSPECTOR_HEADER_TAG, p1)
        c1 = _wait_cursor(tf, _INSPECTOR_PANEL_TAG, p1)
        _assert_cursor_at("inspector#1", c1, p1)
        # The panel did NOT tear off — still docked in the main window.
        main_after = tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window="main")
        assert _scene_contains_tag(main_after, _INSPECTOR_PANEL_TAG), (
            "in-rect release must snap back (inspector stays docked), not tear off"
        )
        assert not _scene_contains_tag(main_after, "inspector_placeholder"), (
            "no placeholder — the inspector was not torn off"
        )
        # tear_off_fired stayed false.
        assert _query(tf, f"/{_INSPECTOR_PANEL_TAG}/external/tear_off_fired") is False

        # ── (C) tracks distinct release points ────────────────────
        p2 = (round(ins[0] + ins[2] * 0.4), round(ins[1] + ins[3] * 0.7))
        assert p2 != p1, "the second target must differ"
        _assert_in_rect("p2", p2, ins)
        _drag_to(tf, _INSPECTOR_HEADER_TAG, p2)
        c2 = _wait_cursor(tf, _INSPECTOR_PANEL_TAG, p2)
        _assert_cursor_at("inspector#2", c2, p2)
        assert list(c2) != list(c1), (
            f"drag_cursor must update to the new release point; stale={c1!r} new={c2!r}"
        )

        # ── (D) per-panel independence ────────────────────────────
        prop = _panel_rect(tf, _PROPERTY_PANEL_TAG)
        assert prop[2] > 50 and prop[3] > 50, f"property rect must be sized; got {prop!r}"
        pp = (round(prop[0] + prop[2] * 0.5), round(prop[1] + prop[3] * 0.5))
        _assert_in_rect("pp", pp, prop)
        _drag_to(tf, _PROPERTY_HEADER_TAG, pp)
        cp = _wait_cursor(tf, _PROPERTY_PANEL_TAG, pp)
        _assert_cursor_at("property#1", cp, pp)
        # The inspector's drag_cursor is unchanged by a property drag — each
        # panel external owns its own slot.
        ins_still = _drag_cursor(tf, _INSPECTOR_PANEL_TAG)
        _assert_cursor_at("inspector#unchanged", ins_still, p2)
        assert list(cp) != list(ins_still), (
            f"property + inspector drag_cursors are independent; got {cp!r} == {ins_still!r}"
        )

        # ── (E) read-only (router-driven, not AI-writable) ────────
        try:
            tf.request(
                "scene/intervene",
                {"path": f"/{_INSPECTOR_PANEL_TAG}/external/drag_cursor", "value": [1.0, 2.0]},
            )
            raised = False
        except RpcError:
            raised = True
        assert raised, "intervene on drag_cursor must be rejected (read-only slot)"
        # And the value is untouched by the rejected intervene.
        _assert_cursor_at("inspector#post-intervene", _drag_cursor(tf, _INSPECTOR_PANEL_TAG), p2)


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1093 §5.15 §5.51 §2 #7 — live drag cursor as scene-as-data",
        body,
    ))
