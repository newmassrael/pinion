#!/usr/bin/env python3
"""R692 §5.16 §5.40 §5.50 — Toolbar widget first consumer end-to-end.

Drives the new `hello-toolbar` binding via JSON-RPC + verifies the
`pinion_widget_paint::toolbar` substrate over a command-class
`ToolbarExternal`: the roving-tabindex control strip, command vs toggle
control classes, click-activation (commands fire `"command"`, toggles
flip a persistent pressed bit + fire `"toggle"`), the WAI-ARIA 1.2 §3.4
toolbar keyboard model, and the roving-cursor focus-ring paint.

The toolbar groups two control classes:
  - command (Undo / Redo) — one-shot, no persistent state;
  - toggle  (Bold / Italic / Underline) — `aria-pressed`, persistent.

R692 atomic verification scope (>=30 assertions):

  (A) substrate shape — toolbar tag `toolbar` + control tags
      `toolbar#0..4`; control labels render; initial query state.
  (B) toggle click — clicking Bold flips its pressed bit + paints the
      tonal fill + fires `toolbar.toggle` (payload `0.true`); re-click
      flips back (`0.false`).
  (C) toggles independent — Italic flips without disturbing Bold.
  (D) command click — clicking Undo focuses it + fires
      `toolbar.command` (payload `3`) with no pressed change.
  (E) keyboard roving — Arrow Left/Right move the cursor (wrap),
      Home/End jump.
  (F) keyboard activate — Enter on a toggle flips it, Space on a
      command fires it.
  (G) focus-ring paint — the roving cursor control draws the accent
      ring; it moves with the cursor.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (560, 220)
LABELS = ["B", "I", "U", "Undo", "Redo"]
PAUSE = 0.12


def _find(node, tag):
    return find_by_tag(node, tag)


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


def _fill_alpha(snap, tag) -> int:
    """Alpha byte of the container tagged `tag`, or 0 when absent /
    fully transparent. A pressed toggle paints a non-zero tonal fill;
    everything else paints transparent (alpha 0)."""
    node = _find(snap, tag)
    if node is None:
        return 0
    fill = (node.get("style") or {}).get("fill") or {}
    return int(fill.get("a") or 0)


def _is_filled(snap, tag) -> bool:
    return _fill_alpha(snap, tag) != 0


def _has_focus_ring(snap, tag) -> bool:
    """True when the control tagged `tag` carries a non-zero-width
    border — the roving-cursor focus ring."""
    node = _find(snap, tag)
    if node is None:
        return False
    border = (node.get("style") or {}).get("border")
    if not isinstance(border, dict):
        return False
    return int(border.get("width") or 0) > 0


def _assert_intent(tf, kind: str, payload: str, label: str) -> None:
    """A control activation drains through the shell's per-dispatch
    intent walk, which logs `shell: intent toolbar.<kind> payload=..`
    to stderr before the `scene/intents` RPC poll can race it (the
    §5.20 single-consumer drain, see `handle_tail`). So the stderr log
    is the observable channel for input-driven activation intents."""
    needle = f'shell: intent toolbar.{kind} payload=Text("{payload}")'
    tail = tf.stderr_tail(80)
    assert any(needle in ln for ln in tail), (
        f"{label}: {kind} {payload!r} not logged; recent stderr={tail[-6:]!r}"
    )


def body() -> None:
    with RpcSubprocess("hello-toolbar", boot_grace=1.5) as tf:
        # ── (A) substrate shape + initial state ─────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _find(snap, "toolbar") is not None, "toolbar must carry the toolbar tag"
        for i in range(5):
            assert _find(snap, f"toolbar#{i}") is not None, f"control toolbar#{i} must exist"

        assert_eq(tf.query("/external/count"), 5, "count")
        assert_eq(tf.query("/external/focus"), 0, "initial focus = 0")
        assert_eq(tf.query("/external/kind.0"), "toggle", "control 0 is a toggle")
        assert_eq(tf.query("/external/kind.3"), "command", "control 3 is a command")
        assert_eq(tf.query("/external/pressed.0"), False, "Bold starts unpressed")

        bar_text = _all_text(_find(snap, "toolbar"))
        for label in LABELS:
            assert label in bar_text, f"label {label!r} must render; got {bar_text!r}"
        for i in range(5):
            assert not _is_filled(snap, f"toolbar#{i}"), f"control {i} unfilled initially"

        # ── (B) click Bold toggles its pressed bit ──────────────────
        tf.click(path="toolbar#0")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/pressed.0"), True, "click Bold -> pressed")
        assert_eq(tf.query("/external/focus"), 0, "click focuses the control")
        _assert_intent(tf, "toggle", "0.true", "Bold on")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _is_filled(snap, "toolbar#0"), "pressed Bold paints the tonal fill"

        tf.click(path="toolbar#0")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/pressed.0"), False, "re-click Bold -> unpressed")
        _assert_intent(tf, "toggle", "0.false", "Bold off")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _is_filled(snap, "toolbar#0"), "unpressed Bold paints transparent"

        # ── (C) toggles are independent ─────────────────────────────
        tf.click(path="toolbar#1")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/pressed.1"), True, "click Italic -> pressed")
        assert_eq(tf.query("/external/pressed.0"), False, "Italic does not disturb Bold")
        _assert_intent(tf, "toggle", "1.true", "Italic on")
        # Reset Italic for a clean keyboard section.
        tf.click(path="toolbar#1")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/pressed.1"), False, "re-click Italic -> unpressed")

        # ── (D) command click fires command, no pressed change ──────
        tf.click(path="toolbar#3")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 3, "click Undo -> focus 3")
        assert_eq(tf.query("/external/pressed.3"), False, "command has no pressed state")
        _assert_intent(tf, "command", "3", "Undo click")

        # ── (E) WAI-ARIA §3.4 keyboard roving ───────────────────────
        tf.request("focus/set", {"tag": "toolbar"})
        time.sleep(0.1)
        focus = tf.request("focus/get").result
        assert_eq(focus.get("focused"), "toolbar", "toolbar owns focus")

        tf.key(path="toolbar", name="Home")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 0, "Home -> first control")
        tf.key(path="toolbar", name="ArrowRight")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 1, "ArrowRight: 0 -> 1")
        tf.key(path="toolbar", name="ArrowRight")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 2, "ArrowRight: 1 -> 2")
        tf.key(path="toolbar", name="ArrowLeft")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 1, "ArrowLeft: 2 -> 1")
        tf.key(path="toolbar", name="End")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 4, "End -> last control")
        tf.key(path="toolbar", name="ArrowRight")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 0, "ArrowRight wraps: 4 -> 0")
        tf.key(path="toolbar", name="ArrowLeft")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 4, "ArrowLeft wraps: 0 -> 4")

        # ── (F) keyboard activation ─────────────────────────────────
        tf.key(path="toolbar", name="Home")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 0, "back to Bold")
        tf.key(path="toolbar", name="Enter")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/pressed.0"), True, "Enter activates focused toggle")
        _assert_intent(tf, "toggle", "0.true", "keyboard Bold on")
        tf.key(path="toolbar", name="End")
        time.sleep(PAUSE)
        tf.key(path="toolbar", name="Space")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 4, "Space leaves command focused")
        _assert_intent(tf, "command", "4", "keyboard Redo")

        # ── (G) focus-ring paint follows the roving cursor ──────────
        tf.key(path="toolbar", name="Home")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _has_focus_ring(snap, "toolbar#0"), "roving cursor 0 draws the focus ring"
        assert not _has_focus_ring(snap, "toolbar#2"), "non-cursor control has no ring"
        tf.key(path="toolbar", name="ArrowRight")
        tf.key(path="toolbar", name="ArrowRight")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focus"), 2, "cursor at 2")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _has_focus_ring(snap, "toolbar#2"), "ring moved to control 2"
        assert not _has_focus_ring(snap, "toolbar#0"), "ring left control 0"


if __name__ == "__main__":
    sys.exit(run_demo("R692 §5.16 §5.40 §5.50 — Toolbar widget first consumer", body))
