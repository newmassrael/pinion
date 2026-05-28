#!/usr/bin/env python3
"""R691 §5.16 §5.40 §5.50 — Menu widget first consumer end-to-end.

Drives the new `hello-menu` binding via JSON-RPC + verifies the
`pinion_widget_paint::menu` substrate over a command-class
`MenuBarExternal`: menubar shape, click-to-open / re-click-to-close,
menu switching, hover-highlights-without-command, item-activation
emitting a `"command"` intent + dismissing the dropdown, and the
WAI-ARIA 1.2 §3.5 menubar keyboard model.

The menu is command-class (WAI-ARIA `menuitem`), not selection — so the
verification axis is the open/active *structure* + the `"command"`
intent, NOT a persistent `selected_index`.

R691 atomic verification scope (>=30 assertions):

  (A) substrate shape — menubar tag `menu` + title tags `menu#t0..2`;
      no dropdown while closed; titles render their labels.
  (B) click open — clicking a title opens its dropdown (`menu_dropdown`
      + `menu#i0..k`), highlights the title, shows the item labels.
  (C) re-click close — clicking the open title dismisses it.
  (D) switch menus — opening a different title switches the dropdown.
  (E) hover — `i<i>:PointerEnter` highlights the active item without
      firing a command.
  (F) activate — clicking an item emits the `menu.command` intent
      (payload `<menu>.<item>`) and closes the menu.
  (G) keyboard — focus the menubar, then Arrow Left/Right titles +
      wrap, Arrow Down open, Up/Down item nav + wrap, Home/End,
      Right/Left switch menus, Escape close, Enter activate.
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

VIEWPORT = (520, 320)
TITLES = ["File", "Edit", "View"]
ITEMS = [
    ["New", "Open", "Save", "Quit"],
    ["Undo", "Redo", "Cut", "Copy", "Paste"],
    ["Zoom In", "Zoom Out", "Reset Zoom"],
]
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
    fully transparent. The open title + active item paint a non-zero
    fill; everything else paints transparent (alpha 0)."""
    node = _find(snap, tag)
    if node is None:
        return 0
    fill = (node.get("style") or {}).get("fill") or {}
    return int(fill.get("a") or 0)


def _is_filled(snap, tag) -> bool:
    return _fill_alpha(snap, tag) != 0


def _assert_command(tf, payload: str, label: str) -> None:
    """A command activation drains through the shell's per-dispatch
    intent walk, which logs `shell: intent menu.command payload=Text(..)`
    to stderr before the `scene/intents` RPC poll can race it (the §5.20
    single-consumer drain, see `handle_tail`). So the stderr log is the
    observable channel for input-driven command intents."""
    needle = f'shell: intent menu.command payload=Text("{payload}")'
    tail = tf.stderr_tail(80)
    assert any(needle in ln for ln in tail), (
        f"{label}: command {payload!r} not logged; recent stderr={tail[-6:]!r}"
    )


def body() -> None:
    with RpcSubprocess("hello-menu", boot_grace=1.5) as tf:
        # ── (A) substrate shape + initial closed state ──────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _find(snap, "menu") is not None, "menubar must carry the menu tag"
        for m in range(3):
            assert _find(snap, f"menu#t{m}") is not None, f"title menu#t{m} must exist"
        assert _find(snap, "menu_dropdown") is None, "no dropdown while closed"

        assert_eq(tf.query("/external/menu_count"), 3, "menu_count")
        assert_eq(tf.query("/external/open"), None, "initial open = none")
        assert_eq(tf.query("/external/active"), None, "initial active = none")
        assert_eq(tf.query("/external/bar_focus"), 0, "initial bar_focus = 0")

        bar_text = _all_text(_find(snap, "menu"))
        for title in TITLES:
            assert title in bar_text, f"title {title!r} must render; got {bar_text!r}"
        for m in range(3):
            assert not _is_filled(snap, f"menu#t{m}"), f"title {m} not highlighted while closed"

        # ── (B) click a title opens its dropdown ────────────────────
        tf.click(path="menu#t0")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(tf.query("/external/open"), 0, "click File -> open 0")
        assert _find(snap, "menu_dropdown") is not None, "dropdown appears when open"
        for i in range(len(ITEMS[0])):
            assert _find(snap, f"menu#i{i}") is not None, f"item menu#i{i} must exist"
        assert _is_filled(snap, "menu#t0"), "open title is highlighted"
        dd_text = _all_text(_find(snap, "menu_dropdown"))
        for item in ITEMS[0]:
            assert item in dd_text, f"item {item!r} must render; got {dd_text!r}"

        # ── (C) re-click the open title closes it ───────────────────
        tf.click(path="menu#t0")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(tf.query("/external/open"), None, "re-click File -> closed")
        assert _find(snap, "menu_dropdown") is None, "dropdown gone after close"

        # ── (D) switch menus ────────────────────────────────────────
        tf.click(path="menu#t1")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), 1, "click Edit -> open 1")
        tf.click(path="menu#t2")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(tf.query("/external/open"), 2, "click View -> switch to open 2")
        dd_text = _all_text(_find(snap, "menu_dropdown"))
        assert "Zoom In" in dd_text, "switched dropdown shows View items"
        tf.click(path="menu#t2")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), None, "re-click View -> closed")

        # ── (E) hover highlights the active item, no activation ─────
        tf.click(path="menu#t0")
        time.sleep(PAUSE)
        tf.invoke("/external/send", "i2:PointerEnter")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 2, "hover item 2 -> active 2")
        assert_eq(tf.query("/external/open"), 0, "hover does not activate (menu stays open)")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _is_filled(snap, "menu#i2"), "active item is highlighted"
        assert not _is_filled(snap, "menu#i0"), "non-active item not highlighted"

        # ── (F) clicking an item fires command + closes ─────────────
        tf.click(path="menu#i1")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), None, "item click closes the menu")
        _assert_command(tf, "0.1", "item click")

        # ── (G) WAI-ARIA §3.5 keyboard model ────────────────────────
        tf.request("focus/set", {"tag": "menu"})
        time.sleep(0.1)
        focus = tf.request("focus/get").result
        assert_eq(focus.get("focused"), "menu", "menubar owns focus")

        tf.key(path="menu", name="ArrowRight")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/bar_focus"), 1, "ArrowRight: bar_focus 0 -> 1")
        tf.key(path="menu", name="ArrowRight")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/bar_focus"), 2, "ArrowRight: 1 -> 2")
        tf.key(path="menu", name="ArrowRight")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/bar_focus"), 0, "ArrowRight wraps: 2 -> 0")
        assert_eq(tf.query("/external/open"), None, "arrow nav while closed never opens")

        tf.key(path="menu", name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), 0, "ArrowDown opens focused menu")
        assert_eq(tf.query("/external/active"), 0, "open highlights first item")
        tf.key(path="menu", name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 1, "ArrowDown: item 0 -> 1")
        tf.key(path="menu", name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 0, "ArrowUp: item 1 -> 0")
        tf.key(path="menu", name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 3, "ArrowUp wraps: 0 -> last (3)")
        tf.key(path="menu", name="Home")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 0, "Home -> first item")
        tf.key(path="menu", name="End")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 3, "End -> last item")

        tf.key(path="menu", name="ArrowRight")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), 1, "ArrowRight in menu -> switch to Edit")
        assert_eq(tf.query("/external/active"), 0, "switched menu highlights first")
        tf.key(path="menu", name="ArrowLeft")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), 0, "ArrowLeft in menu -> back to File")

        tf.key(path="menu", name="Escape")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), None, "Escape closes the menu")

        # Keyboard activation emits the command intent.
        tf.key(path="menu", name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), 0, "ArrowDown reopens File")
        tf.key(path="menu", name="Enter")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), None, "Enter activates + closes")
        _assert_command(tf, "0.0", "keyboard Enter")


if __name__ == "__main__":
    sys.exit(run_demo("R691 §5.16 §5.40 §5.50 — Menu widget first consumer", body))
