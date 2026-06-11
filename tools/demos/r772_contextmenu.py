#!/usr/bin/env python3
"""R772 §5.53 §5.38 — ContextMenu (own-renderer right-click popup) E2E.

Drives the new `hello-contextmenu` binding via JSON-RPC + verifies the
`pinion_widget_paint::view_context_menu` substrate over a command-class
`ContextMenuExternal`: closed-initial shape, `open_at` anchoring the
popup at an arbitrary cursor point, on-screen clamping near the window
edges, hover-highlights-without-command, item-activation emitting a
`"command"` intent + dismissing, the R715 click-outside dismiss barrier,
programmatic `intervene` close/highlight, and the WAI-ARIA 1.2 §3.5 menu
keyboard model.

Per R771.1 (§5.53) the popup is drawn by pinion's OWN scene-graph
renderer on every platform — so its position/structure is fully
introspectable as scene-as-data (§2 invariant #7), no native menu
opacity. The native secondary-button path (`apply_secondary_click`) is
verified live by `tools/` XTEST separately; here the open path is the
AI-first `open_at` invoke (the binding-specific programmatic action).
Since R887 the universal §2 invariant #2 input peer is `scene/click
{button: "right"}` — exercised by `r887_secondary_click.py`.

The menu is command-class (WAI-ARIA `menuitem`), not selection — so the
verification axis is the open/active *structure* + the `"command"`
intent, NOT a persistent `selected_index`.

R772 atomic verification scope (>=30 assertions):

  (A) substrate shape — closed: no popup panel `ctx`, no barrier,
      no item rows; item_count = 4; body instruction renders.
  (B) open_at — `open_at` opens the popup at the cursor; panel `ctx` +
      item rows `ctx#i0..3` appear at the anchor, labels render,
      open/open_x/open_y reflect the point, MD3 L2 elevation shadows.
  (C) clamp — open_at near the bottom-right edge slides the panel back
      fully on-screen.
  (D) re-anchor — a second open_at moves the popup, clears highlight.
  (E) hover — `i<i>:PointerEnter` highlights the active item without
      firing a command; the painted row fills.
  (F) activate (click) — clicking an item emits `ctx.command` (payload
      `<i>`) and closes the popup.
  (G) barrier — `barrier:PointerUp` (click-outside) dismisses.
  (H) intervene — `open=false` closes; `active=i` highlights silently.
  (I) keyboard — focus the menu, Arrow Up/Down item nav + wrap,
      Home/End, Enter activate (command + close), Escape close.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (520, 360)
ITEMS = ["Cut", "Copy", "Paste", "Delete"]
# Cursor target for keyboard injection. `scene/key` moves the cursor to
# this point before dispatching the (named) key to the focused widget; a
# top-left corner sits over the dismiss barrier, *not* over a popup item,
# so the move fires no item `PointerEnter` that would pollute `active`
# (the popup opens around (100,100), well clear of this point).
KEY_AT = (5.0, 5.0)


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
    fully transparent. The active item paints a non-zero fill;
    everything else paints transparent (alpha 0)."""
    node = _find(snap, tag)
    if node is None:
        return 0
    fill = (node.get("style") or {}).get("fill") or {}
    return int(fill.get("a") or 0)


def _is_filled(snap, tag) -> bool:
    return _fill_alpha(snap, tag) != 0


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _wait_paint(tf, predicate, desc):
    """Poll the paint scene until `predicate(snap)` holds (R705 race-safe);
    returns the satisfying snapshot."""
    return wait_until(
        lambda: (lambda s: s if predicate(s) else None)(_paint(tf)),
        desc=desc,
    )


def _assert_command(tf, payload: str, label: str) -> None:
    """A command activation drains through the shell's per-dispatch
    intent walk, logging `shell: intent ctx.command payload=Text(..)` to
    stderr before a `scene/intents` poll can race it (§5.20 single-
    consumer drain). So the stderr log is the observable channel for
    input-driven command intents (mirrors r691_menu)."""
    needle = f'shell: intent ctx.command payload=Text("{payload}")'
    tail = tf.stderr_tail(80)
    assert any(needle in ln for ln in tail), (
        f"{label}: command {payload!r} not logged; recent stderr={tail[-6:]!r}"
    )


def body() -> None:
    with RpcSubprocess("hello-contextmenu", boot_grace=1.5) as tf:
        # ── (A) substrate shape + initial closed state ──────────────
        snap = _paint(tf)
        assert _find(snap, "ctx") is None, "no popup panel while closed"
        assert _find(snap, "ctx#barrier") is None, "no dismiss barrier while closed"
        assert _find(snap, "ctx#i0") is None, "no item rows while closed"
        assert_eq(tf.query("/external/item_count"), 4, "item_count")
        assert_eq(tf.query("/external/open"), False, "initial open = false")
        assert_eq(tf.query("/external/active"), None, "initial active = none")
        body_text = " ".join(_all_text(snap))
        assert "Right-click" in body_text, f"instruction renders; got {body_text!r}"

        # ── (B) open_at anchors the popup at the cursor point ───────
        assert_eq(tf.invoke("/external/open_at", "120,90"), True, "open_at returns open=true")
        assert_eq(tf.query("/external/open"), True, "open_at -> open")
        assert_eq(tf.query("/external/open_x"), 120.0, "open_x = anchor x")
        assert_eq(tf.query("/external/open_y"), 90.0, "open_y = anchor y")
        assert_eq(tf.query("/external/active"), None, "pointer open pre-highlights nothing")
        snap = _wait_paint(tf, lambda s: _find(s, "ctx") is not None, "popup paints after open_at")
        for i in range(len(ITEMS)):
            assert _find(snap, f"ctx#i{i}") is not None, f"item ctx#i{i} must exist"
        assert _find(snap, "ctx#barrier") is not None, "dismiss barrier paints when open"
        panel_text = _all_text(_find(snap, "ctx"))
        for item in ITEMS:
            assert item in panel_text, f"item {item!r} must render; got {panel_text!r}"
        rects = abs_rects_of(snap)
        px, py, pw, ph = rects["ctx"]
        assert px == 120 and py == 90, f"panel anchored at (120,90); got ({px},{py})"
        # R711 MD3 elevation — the popup floats at Level 2 (key + ambient
        # drop-shadow). Read it back as data (§2 #7); R710 guards the cast.
        shadows = (_find(snap, "ctx").get("style") or {}).get("shadows") or []
        assert len(shadows) == 2, f"popup casts key + ambient (MD3 L2); got {shadows}"
        assert abs(shadows[0]["blur"] - 3.0) < 1e-3, f"L2 key blur 3.0; got {shadows[0]}"
        assert abs(shadows[0]["offset"]["y"] - 2.0) < 1e-4, "L2 key offset.y 2"

        # ── (C) clamp: open near the bottom-right edge slides back ──
        assert_eq(tf.invoke("/external/open_at", "510,350"), True, "open_at near edge")
        snap = _wait_paint(
            tf,
            lambda s: abs_rects_of(s).get("ctx", (0, 0, 0, 0))[0] < 510,
            "panel slides back on-screen",
        )
        cx, cy, cw, ch = abs_rects_of(snap)["ctx"]
        assert cx + cw <= VIEWPORT[0], f"clamped panel right edge in-bounds: {cx}+{cw}>{VIEWPORT[0]}"
        assert cy + ch <= VIEWPORT[1], f"clamped panel bottom edge in-bounds: {cy}+{ch}>{VIEWPORT[1]}"
        assert cx < 510 and cy < 350, "panel slid back from the near-edge anchor"
        # The model keeps the raw anchor (the painter does the clamping).
        assert_eq(tf.query("/external/open_x"), 510.0, "model retains the raw anchor x")

        # ── (D) re-anchor moves the popup + clears the highlight ────
        tf.invoke("/external/send", "i1:PointerEnter")
        assert_eq(tf.query("/external/active"), 1, "hover sets active before re-anchor")
        assert_eq(tf.invoke("/external/open_at", "60,40"), True, "re-anchor open_at")
        assert_eq(tf.query("/external/open_x"), 60.0, "re-anchor moved open_x")
        assert_eq(tf.query("/external/active"), None, "re-anchor clears the highlight")

        # ── (E) hover highlights the active item, no activation ─────
        tf.invoke("/external/send", "i2:PointerEnter")
        assert_eq(tf.query("/external/active"), 2, "hover item 2 -> active 2")
        assert_eq(tf.query("/external/open"), True, "hover does not activate (popup stays open)")
        snap = _wait_paint(tf, lambda s: _is_filled(s, "ctx#i2"), "active item fills")
        assert _is_filled(snap, "ctx#i2"), "active item is highlighted"
        assert not _is_filled(snap, "ctx#i0"), "non-active item not highlighted"

        # ── (F) clicking an item fires command + closes ─────────────
        tf.click(path="ctx#i2")
        wait_until(lambda: tf.query("/external/open") is False, desc="item click closes popup")
        assert_eq(tf.query("/external/open"), False, "item click closes the popup")
        _assert_command(tf, "2", "item click")

        # The mouse click left the physical cursor over the (now closed)
        # popup region; reopening there would hover whichever item lands
        # under it (real UX, but non-deterministic for the introspection-
        # only sections below). Reset to "no pointer over the window" so
        # `active` is governed solely by explicit send / key / intervene.
        tf.pointer_leave()

        # ── (G) click-outside barrier dismisses ─────────────────────
        assert_eq(tf.invoke("/external/open_at", "100,100"), True, "reopen for barrier test")
        assert_eq(tf.query("/external/open"), True, "popup open before barrier click")
        tf.invoke("/external/send", "barrier:PointerUp")
        assert_eq(tf.query("/external/open"), False, "barrier PointerUp (click-outside) dismisses")

        # ── (H) programmatic intervene close / highlight ────────────
        tf.invoke("/external/open_at", "100,100")
        tf.intervene("/external/active", 3)
        assert_eq(tf.query("/external/active"), 3, "intervene active=3 highlights")
        tf.intervene("/external/open", False)
        assert_eq(tf.query("/external/open"), False, "intervene open=false closes")

        # ── (I) WAI-ARIA §3.5 menu keyboard model ───────────────────
        tf.invoke("/external/open_at", "100,100")
        tf.request("focus/set", {"tag": "ctx"})
        focus = wait_until(
            lambda: (lambda f: f if f.get("focused") == "ctx" else None)(
                tf.request("focus/get").result
            ),
            desc="menu owns focus",
        )
        assert_eq(focus.get("focused"), "ctx", "context menu owns focus")

        tf.key(at=KEY_AT, name="ArrowDown")
        wait_until(lambda: tf.query("/external/active") == 0, desc="ArrowDown -> first item")
        assert_eq(tf.query("/external/active"), 0, "ArrowDown from none -> item 0")
        tf.key(at=KEY_AT, name="ArrowDown")
        wait_until(lambda: tf.query("/external/active") == 1, desc="ArrowDown -> 1")
        assert_eq(tf.query("/external/active"), 1, "ArrowDown: 0 -> 1")
        tf.key(at=KEY_AT, name="ArrowUp")
        wait_until(lambda: tf.query("/external/active") == 0, desc="ArrowUp -> 0")
        assert_eq(tf.query("/external/active"), 0, "ArrowUp: 1 -> 0")
        tf.key(at=KEY_AT, name="ArrowUp")
        wait_until(lambda: tf.query("/external/active") == 3, desc="ArrowUp wraps -> last")
        assert_eq(tf.query("/external/active"), 3, "ArrowUp wraps: 0 -> last (3)")
        tf.key(at=KEY_AT, name="Home")
        wait_until(lambda: tf.query("/external/active") == 0, desc="Home -> first")
        assert_eq(tf.query("/external/active"), 0, "Home -> first item")
        tf.key(at=KEY_AT, name="End")
        wait_until(lambda: tf.query("/external/active") == 3, desc="End -> last")
        assert_eq(tf.query("/external/active"), 3, "End -> last item")
        assert_eq(tf.query("/external/open"), True, "nav keeps the popup open")

        # Keyboard activation emits the command intent + closes.
        tf.key(at=KEY_AT, name="Home")
        wait_until(lambda: tf.query("/external/active") == 0, desc="Home before Enter")
        tf.key(at=KEY_AT, name="Enter")
        wait_until(lambda: tf.query("/external/open") is False, desc="Enter activates + closes")
        assert_eq(tf.query("/external/open"), False, "Enter activates + closes")
        _assert_command(tf, "0", "keyboard Enter")

        # Escape closes without firing a command.
        tf.invoke("/external/open_at", "100,100")
        tf.request("focus/set", {"tag": "ctx"})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == "ctx",
            desc="refocus before Escape",
        )
        tf.key(at=KEY_AT, name="Escape")
        wait_until(lambda: tf.query("/external/open") is False, desc="Escape closes")
        assert_eq(tf.query("/external/open"), False, "Escape closes the popup")


if __name__ == "__main__":
    sys.exit(run_demo("R772 §5.53 §5.38 — ContextMenu own-renderer popup", body))
