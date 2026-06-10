#!/usr/bin/env python3
"""R690 §5.16 §5.40 §5.50 — Tabs widget first consumer end-to-end.

Drives the new `hello-tabs` binding via JSON-RPC + verifies the
`pinion_widget_paint::tabs` substrate over a reused
`RadioGroupExternal` selection model: tab-strip shape, active-indicator
tracking, per-tab panel swap, click-to-select, and the WAI-ARIA
automatic-activation keyboard model (Arrow Left/Right roving with wrap,
Home/End jump) after focusing the TabList via `focus/set`.

R690 atomic 4 verification scope (>=30 assertions):

  (A) substrate shape — strip tag `tabs` + composite tab tags
      `tabs#0..2` + panel tag `tabs_panel`.
  (B) initial boot — tab 0 active (indicator filled), tabs 1/2 inactive
      (indicator transparent), panel shows the General body.
  (C) click selection — clicking a tab activates it (selected_index +
      indicator move) and swaps the panel body.
  (D) AI introspect — `/external/selected_index` mirrors the visible
      selection.
  (E) keyboard roving — focus the TabList, then Arrow Right/Left cycle
      with wrap-around + Home/End jump (automatic activation).
  (F) labels — all three tab labels render in the strip.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_until,
)

VIEWPORT = (520, 320)
LABELS = ["General", "Appearance", "Advanced"]
# Distinct lead word per tab body so the panel swap is observable.
PANEL_LEAD = ["Language", "Theme", "Developer"]


def _find(node, tag):
    return find_by_tag(node, tag)


def _indicator_active(tab_node) -> bool:
    """A tab's active-indicator Box has a non-zero alpha fill; the
    inactive band paints fully transparent (alpha 0)."""

    def box_fill(n):
        if isinstance(n, dict):
            if n.get("type") == "Box":
                return n.get("style", {}).get("fill")
            for c in n.get("children") or []:
                found = box_fill(c)
                if found is not None:
                    return found
        return None

    fill = box_fill(tab_node) or {}
    return int(fill.get("a") or 0) != 0


def _all_text(node) -> list[str]:
    out: list[str] = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text":
                content = n.get("content")
                if isinstance(content, str):
                    out.append(content)
            for c in n.get("children") or []:
                walk(c)

    walk(node)
    return out


def _panel_text(snap) -> str:
    panel = _find(snap, "tabs_panel")
    assert panel is not None, "tabs_panel node must exist"
    texts = _all_text(panel)
    return " ".join(texts)


def _selected_indicator_index(snap) -> int | None:
    """Return the index whose tab carries the active indicator, or
    None if no tab is active."""
    active = [i for i in range(len(LABELS)) if _indicator_active(_find(snap, f"tabs#{i}"))]
    if not active:
        return None
    assert len(active) == 1, f"exactly one tab may carry the indicator; got {active}"
    return active[0]


def body() -> None:
    with RpcSubprocess("hello-tabs", boot_grace=1.5) as tf:
        # ── (A) substrate shape ────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _find(snap, "tabs") is not None, "strip must carry the tabs tag"
        for i in range(3):
            assert _find(snap, f"tabs#{i}") is not None, f"tab tabs#{i} must exist"
        assert _find(snap, "tabs_panel") is not None, "panel must carry tabs_panel tag"

        # ── (B) initial boot: tab 0 active ──────────────────────────
        assert_eq(tf.query("/external/selected_index"), 0, "initial selected_index")
        assert_eq(_selected_indicator_index(snap), 0, "initial indicator on tab 0")
        assert PANEL_LEAD[0] in _panel_text(snap), (
            f"initial panel must show General body; got {_panel_text(snap)!r}"
        )
        assert PANEL_LEAD[2] not in _panel_text(snap), "initial panel must not show Advanced body"

        # ── (F) labels render ───────────────────────────────────────
        strip_text = _all_text(_find(snap, "tabs"))
        for label in LABELS:
            assert label in strip_text, f"tab label {label!r} must render; got {strip_text!r}"

        # ── (C) click selection swaps indicator + panel ─────────────
        tf.click(path="tabs#2")
        wait_query(tf, "/external/selected_index", 2, desc="click tab 2 -> selected 2")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(_selected_indicator_index(snap), 2, "indicator moved to tab 2")
        assert PANEL_LEAD[2] in _panel_text(snap), "panel shows Advanced body after click"
        assert PANEL_LEAD[0] not in _panel_text(snap), "panel no longer shows General body"

        tf.click(path="tabs#1")
        wait_query(tf, "/external/selected_index", 1, desc="click tab 1 -> selected 1")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(_selected_indicator_index(snap), 1, "indicator moved to tab 1")
        assert PANEL_LEAD[1] in _panel_text(snap), "panel shows Appearance body after click"

        tf.click(path="tabs#0")
        wait_query(tf, "/external/selected_index", 0, desc="click tab 0 -> selected 0")

        # ── (E) keyboard roving (automatic activation) ──────────────
        # Focus the TabList (the WAI-ARIA single tab stop) the way a
        # Tab keypress would, then drive Arrow/Home/End.
        tf.request("focus/set", {"tag": "tabs"})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == "tabs",
            desc="TabList owns focus",
        )

        tf.key(path="tabs", name="ArrowRight")
        wait_query(tf, "/external/selected_index", 1, desc="ArrowRight: 0 -> 1")

        tf.key(path="tabs", name="ArrowRight")
        wait_query(tf, "/external/selected_index", 2, desc="ArrowRight: 1 -> 2")

        tf.key(path="tabs", name="ArrowRight")
        wait_query(tf, "/external/selected_index", 0, desc="ArrowRight wraps: 2 -> 0")

        tf.key(path="tabs", name="ArrowLeft")
        wait_query(tf, "/external/selected_index", 2, desc="ArrowLeft wraps: 0 -> 2")

        tf.key(path="tabs", name="Home")
        wait_query(tf, "/external/selected_index", 0, desc="Home -> first tab")

        tf.key(path="tabs", name="End")
        wait_query(tf, "/external/selected_index", 2, desc="End -> last tab")

        # Keyboard selection reflects in the paint indicator too.
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(_selected_indicator_index(snap), 2, "indicator follows keyboard to tab 2")
        assert PANEL_LEAD[2] in _panel_text(snap), "panel follows keyboard to Advanced body"

        tf.key(path="tabs", name="ArrowLeft")
        wait_query(tf, "/external/selected_index", 1, desc="ArrowLeft: 2 -> 1")


if __name__ == "__main__":
    sys.exit(run_demo("R690 §5.16 §5.40 §5.50 — Tabs widget first consumer", body))
