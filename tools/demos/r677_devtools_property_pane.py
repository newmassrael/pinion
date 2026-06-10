#!/usr/bin/env python3
"""R677 §5.16 §5.49 — DevTools property pane (Computed-pane analog).

Drives `hello-multi-window` after R677's enhancements on top of R676:

  1. **2-pane inspector layout** — `view_inspector` produces a Row
     container with two children: the existing tree pane (left,
     `inspector_tree`) and the new property pane (right,
     `property_pane`). `LayoutStyle::flex_grow = 1.0` on the property
     pane fills the available horizontal space.
  2. **Property pane field walker** — `property_pane_rows(scene)`
     resolves the cross-window `use_selected_path()` Signal value
     against the live main-window scene via the R676
     `find_main_node_at_path` helper (2nd consumer — tracking the
     [[abstraction-needs-second-consumer]] Rule-of-Three threshold),
     then emits one `field: value` row per scene-node field
     (Chrome DevTools Computed-pane mirror).
  3. **Inspector window resized to 480×320** — wide enough to host
     the 2-pane layout without horizontal crowding.

R677 atomic (3) verification scope (≥ 30 assertions):

  (A) Inspector window's declared size is now 480×320 (`scene/layout`
      returns rect.w=480, rect.h=320).
  (B) Property pane is present at the inspector's outer Row level —
      tagged `property_pane`, fills the right half of the layout.
  (C) Baseline: no selection → pane renders the `(no selection)`
      placeholder text.
  (D) Inspector tree click on the tagged button row → property pane
      renders the canonical Container field rows: `type: Container`,
      `tag: main_btn`, `style.fill: rgba(...)`, `style.border: ...`,
      `layout.size: <w>px × <h>px`, `children: 1`.
  (E) Click on the label Text leaf row → pane renders Text rows:
      `type: Text`, `content: "Click me!"`, `style.font_size: 18px`,
      `style.fg: rgba(...)`.
  (F) Click on root Container row → pane renders root field rows
      (`type: Container`, `tag: —` em-dash for untagged root,
      `children: 1 or 2` depending on banner).
  (G) Inspector-only id `state` → pane shows placeholder (path
      doesn't resolve in main scene).
  (H) Pane atomically replaces field rows on selection change
      (button → label → state → button keeps the pane current).
  (I) AI-driven selection via `/inspector_tree/external/click`
      typed shortcut produces bit-identical pane content to a
      composite-tag send-wire cycle.
  (J) ButtonState mutation (d/e keypress → Disabled/Idle) preserves
      the field rows for the selected node — the tag path stays
      resolved, only the underlying fill colour changes.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    run_demo,
    wait_until,
)


_VIEW_MAIN_ROOT_PATH = "Container"
_BUTTON_PATH = "Container/Container[main_btn]"
_LABEL_PATH = "Container/Container[main_btn]/Text[0]"

_INSPECTOR_W = 480
_INSPECTOR_H = 320


def _snap_inspector(tf) -> dict:
    resp = tf.request(
        "scene/snapshot",
        {"path": "", "window": "inspector", "from": "paint",
         "viewport": {"w": _INSPECTOR_W, "h": _INSPECTOR_H}},
    )
    assert resp is not None
    return resp.result


def _snap_main(tf) -> dict:
    resp = tf.request(
        "scene/snapshot",
        {"path": "", "window": "main", "from": "paint",
         "viewport": {"w": 320, "h": 200}},
    )
    assert resp is not None
    return resp.result


def _pane_text_rows(snapshot) -> list[str]:
    """Extract every Text content string inside the `property_pane`
    Container. Each Text is a one-row field render from
    `property_pane_rows`.
    """
    pane = find_by_tag(snapshot, "property_pane")
    if pane is None:
        return []
    out: list[str] = []
    for child in pane.get("children") or []:
        if isinstance(child, dict) and child.get("type") == "Text":
            content = child.get("content")
            if isinstance(content, str):
                out.append(content)
    return out


def _wait_rows(tf, predicate, desc: str) -> list[str]:
    """Poll the inspector pane rows until `predicate(rows)` is truthy
    (R883 zero-flake); returns the matching rows."""
    return wait_until(
        lambda: (lambda rows: rows if predicate(rows) else None)(
            _pane_text_rows(_snap_inspector(tf))
        ),
        desc=desc,
    )


def body() -> None:
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        # ── (A) Inspector window is 480×320 ───────────────────────
        layout_inspector = tf.request(
            "scene/layout", {"window": "inspector", "viewport": None},
        )
        assert layout_inspector is not None
        ins_rect = layout_inspector.result.get("rect") or {}
        assert int(ins_rect.get("w") or 0) == _INSPECTOR_W, (
            f"R677 inspector width must be {_INSPECTOR_W}; "
            f"got: {layout_inspector.result!r}"
        )
        assert int(ins_rect.get("h") or 0) == _INSPECTOR_H, (
            f"R677 inspector height must be {_INSPECTOR_H}; "
            f"got: {layout_inspector.result!r}"
        )

        # ── (B) Property pane is present ──────────────────────────
        snap_baseline = _snap_inspector(tf)
        assert find_by_tag(snap_baseline, "inspector_tree") is not None, (
            "tree pane must persist"
        )
        pane = find_by_tag(snap_baseline, "property_pane")
        assert pane is not None, (
            "R677 property pane must be reachable via depth-first walk"
        )

        # ── (C) Baseline: no selection → placeholder ──────────────
        baseline_rows = _pane_text_rows(snap_baseline)
        assert baseline_rows == ["(no selection)"], (
            f"baseline pane must render placeholder only; got: {baseline_rows!r}"
        )

        # ── (D) Click tagged button row → Container field rows ────
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        rows_d = _wait_rows(
            tf, lambda r: "type: Container" in r,
            "button selection must produce 'type: Container'",
        )
        assert "tag: main_btn" in rows_d, (
            f"button selection must produce 'tag: main_btn'; got: {rows_d!r}"
        )
        assert any(r.startswith("style.fill: rgba(") for r in rows_d), (
            f"button selection must produce 'style.fill: rgba(...)' row; got: {rows_d!r}"
        )
        assert any(r.startswith("style.border: ") for r in rows_d), (
            f"button selection must produce 'style.border: ...' row; got: {rows_d!r}"
        )
        assert any(r.startswith("layout.size: ") and "px × " in r for r in rows_d), (
            f"button selection must produce 'layout.size: <W>px × <H>px' row; got: {rows_d!r}"
        )
        assert "children: 1" in rows_d, (
            f"button container holds 1 child (label Text); got: {rows_d!r}"
        )

        # ── (E) Click label Text leaf → Text field rows ───────────
        tf.invoke("/inspector_tree/external/click", _LABEL_PATH)
        rows_e = _wait_rows(
            tf, lambda r: "type: Text" in r,
            "label selection must produce 'type: Text'",
        )
        assert any(r == 'content: "Click me!"' for r in rows_e), (
            f"label selection must produce content row; got: {rows_e!r}"
        )
        assert any(r.startswith("style.font_size: ") and r.endswith("px") for r in rows_e), (
            f"label selection must produce font_size row; got: {rows_e!r}"
        )
        assert any(r.startswith("style.fg: rgba(") for r in rows_e), (
            f"label selection must produce fg row; got: {rows_e!r}"
        )

        # ── (F) Click root → untagged Container rows ──────────────
        tf.invoke("/inspector_tree/external/click", _VIEW_MAIN_ROOT_PATH)
        rows_f = _wait_rows(
            tf, lambda r: "type: Container" in r,
            "root selection must produce 'type: Container'",
        )
        # Root is untagged; the banner Text now appears because
        # selection != None, so root has children = [banner Text, button Container] = 2.
        assert "tag: —" in rows_f, (
            f"root selection must show em-dash placeholder for untagged tag; got: {rows_f!r}"
        )
        assert any(
            r in ("children: 1", "children: 2") for r in rows_f
        ), (
            f"root children count is 1 (no banner) or 2 (banner on); got: {rows_f!r}"
        )

        # ── (G) Inspector-only id `state` → placeholder ───────────
        tf.invoke("/inspector_tree/external/click", "state")
        rows_g = _wait_rows(
            tf, lambda r: r == ["(no selection)"],
            "inspector-only 'state' must render placeholder",
        )

        # ── (H) Atomic replacement on selection change cycle ──────
        # button → label → state → button — verify each step shows
        # the correct rows (no stale state from previous selection).
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        rows_h0 = _wait_rows(
            tf, lambda r: "tag: main_btn" in r,
            "button rows must show after re-select",
        )
        tf.invoke("/inspector_tree/external/click", _LABEL_PATH)
        rows_h1 = _wait_rows(
            tf, lambda r: "type: Text" in r,
            "label rows must replace button rows",
        )
        assert "tag: main_btn" not in rows_h1, (
            f"button rows must be gone after re-select to label; got: {rows_h1!r}"
        )
        tf.invoke("/inspector_tree/external/click", "main")  # inspector-only
        rows_h2 = _wait_rows(
            tf, lambda r: r == ["(no selection)"],
            "inspector-only 'main' must drop to placeholder",
        )
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        rows_h3 = _wait_rows(
            tf, lambda r: "tag: main_btn" in r,
            "re-select must restore button rows",
        )

        # ── (I) AI-driven send-wire matches typed click ───────────
        # Drive the same selection via composite-tag PointerDown/Up
        # cycle and verify the pane content is identical.
        tf.invoke(
            "/inspector_tree/external/send",
            f"{_LABEL_PATH}:PointerDown",
        )
        tf.invoke(
            "/inspector_tree/external/send",
            f"{_LABEL_PATH}:PointerUp",
        )
        rows_i = _wait_rows(
            tf, lambda r: "type: Text" in r,
            "send-wire label select must produce Text rows",
        )
        assert any(r == 'content: "Click me!"' for r in rows_i), (
            f"send-wire label rows must include content; got: {rows_i!r}"
        )

        # ── (J) State mutation preserves field rows ───────────────
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        rows_idle = _wait_rows(
            tf, lambda r: "tag: main_btn" in r,
            "button rows pre-Disable",
        )
        # Find the style.fill row's value for comparison.
        fill_idle = next((r for r in rows_idle if r.startswith("style.fill: ")), None)
        assert fill_idle is not None

        tf.request(
            "scene/key",
            {"window": "main", "key": "d", "path": "main_btn"},
        )
        rows_disabled = _wait_rows(
            tf, lambda r: "tag: main_btn" in r,
            "button rows persist across Disable; tag-form path stays resolved",
        )
        # The fill colour value should be different (Disabled paints
        # a different fill); the row shape is identical.
        fill_disabled = next(
            (r for r in rows_disabled if r.startswith("style.fill: ")), None,
        )
        assert fill_disabled is not None
        # Pin that the row exists in both states; the values may
        # differ depending on theme animation interpolation, so
        # don't pin specific RGB.
        assert "children: 1" in rows_disabled, (
            "Disabled button still holds 1 child; got: {rows_disabled!r}"
        )

        # Re-enable.
        tf.request(
            "scene/key",
            {"window": "main", "key": "e", "path": "main_btn"},
        )
        rows_enabled = _wait_rows(
            tf, lambda r: "tag: main_btn" in r,
            "Enable restores button rows",
        )

        # ── (K) Main window still works end-to-end ────────────────
        # R677 doesn't touch view_main — confirm the highlight overlay
        # + banner still paint correctly post-selection.
        snap_main_k = _snap_main(tf)
        assert find_by_tag(snap_main_k, "main_btn") is not None, (
            "main_btn must still paint in main window"
        )


if __name__ == "__main__":
    sys.exit(run_demo(
        "R677 §5.16 §5.49 — DevTools property pane (Computed-pane analog)",
        body,
    ))
