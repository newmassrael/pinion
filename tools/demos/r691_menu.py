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
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_stderr,
    wait_until,
)

VIEWPORT = (520, 320)
TITLES = ["File", "Edit", "View"]
ITEMS = [
    ["New", "Open", "Save", "Quit"],
    ["Undo", "Redo", "Cut", "Copy", "Paste"],
    ["Zoom In", "Zoom Out", "Reset Zoom"],
]


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
    observable channel for input-driven command intents — polled, since
    the harness reads stderr through a pump thread (R883 zero-flake)."""
    needle = f'shell: intent menu.command payload=Text("{payload}")'
    wait_stderr(tf, needle, n=80, desc=f"{label}: command {payload!r} logged")


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
        wait_query(tf, "/external/open", 0, desc="click File -> open 0")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _find(snap, "menu_dropdown") is not None, "dropdown appears when open"
        for i in range(len(ITEMS[0])):
            assert _find(snap, f"menu#i{i}") is not None, f"item menu#i{i} must exist"
        assert _is_filled(snap, "menu#t0"), "open title is highlighted"
        dd_text = _all_text(_find(snap, "menu_dropdown"))
        for item in ITEMS[0]:
            assert item in dd_text, f"item {item!r} must render; got {dd_text!r}"
        # R711 MD3 elevation — the dropdown floats at Level 2 (key+ambient
        # drop-shadow from the shared `elevation(2)`). Read it back as data
        # (§2 #7); the cast's pixel fidelity is R710's guard.
        dd_shadows = (_find(snap, "menu_dropdown").get("style") or {}).get("shadows") or []
        assert len(dd_shadows) == 2, f"dropdown casts key + ambient (MD3 L2); got {dd_shadows}"
        # MD3 menu = Level 2 -> key blur = 2 * 1.5 = 3.0, offset.y = 2.
        assert abs(dd_shadows[0]["blur"] - 3.0) < 1e-3, f"L2 key blur 3.0; got {dd_shadows[0]}"
        assert abs(dd_shadows[0]["offset"]["y"] - 2.0) < 1e-4, "L2 key offset.y 2"

        # ── (C) re-click the open title closes it ───────────────────
        tf.click(path="menu#t0")
        wait_query(tf, "/external/open", None, desc="re-click File -> closed")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _find(snap, "menu_dropdown") is None, "dropdown gone after close"

        # ── (D) switch menus ────────────────────────────────────────
        tf.click(path="menu#t1")
        wait_query(tf, "/external/open", 1, desc="click Edit -> open 1")
        tf.click(path="menu#t2")
        wait_query(tf, "/external/open", 2, desc="click View -> switch to open 2")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        dd_text = _all_text(_find(snap, "menu_dropdown"))
        assert "Zoom In" in dd_text, "switched dropdown shows View items"
        tf.click(path="menu#t2")
        wait_query(tf, "/external/open", None, desc="re-click View -> closed")

        # ── (E) hover highlights the active item, no activation ─────
        tf.click(path="menu#t0")
        wait_query(tf, "/external/open", 0, desc="click File -> reopen for hover")
        tf.invoke("/external/send", "i2:PointerEnter")
        wait_query(tf, "/external/active", 2, desc="hover item 2 -> active 2")
        assert_eq(tf.query("/external/open"), 0, "hover does not activate (menu stays open)")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _is_filled(snap, "menu#i2"), "active item is highlighted"
        assert not _is_filled(snap, "menu#i0"), "non-active item not highlighted"

        # ── (F) clicking an item fires command + closes ─────────────
        tf.click(path="menu#i1")
        wait_query(tf, "/external/open", None, desc="item click closes the menu")
        _assert_command(tf, "0.1", "item click")

        # ── (G) WAI-ARIA §3.5 keyboard model ────────────────────────
        tf.request("focus/set", {"tag": "menu"})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == "menu",
            desc="menubar owns focus",
        )

        tf.key(path="menu", name="ArrowRight")
        wait_query(tf, "/external/bar_focus", 1, desc="ArrowRight: bar_focus 0 -> 1")
        tf.key(path="menu", name="ArrowRight")
        wait_query(tf, "/external/bar_focus", 2, desc="ArrowRight: 1 -> 2")
        tf.key(path="menu", name="ArrowRight")
        wait_query(tf, "/external/bar_focus", 0, desc="ArrowRight wraps: 2 -> 0")
        assert_eq(tf.query("/external/open"), None, "arrow nav while closed never opens")

        tf.key(path="menu", name="ArrowDown")
        wait_query(tf, "/external/open", 0, desc="ArrowDown opens focused menu")
        assert_eq(tf.query("/external/active"), 0, "open highlights first item")
        tf.key(path="menu", name="ArrowDown")
        wait_query(tf, "/external/active", 1, desc="ArrowDown: item 0 -> 1")
        tf.key(path="menu", name="ArrowUp")
        wait_query(tf, "/external/active", 0, desc="ArrowUp: item 1 -> 0")
        tf.key(path="menu", name="ArrowUp")
        wait_query(tf, "/external/active", 3, desc="ArrowUp wraps: 0 -> last (3)")
        tf.key(path="menu", name="Home")
        wait_query(tf, "/external/active", 0, desc="Home -> first item")
        tf.key(path="menu", name="End")
        wait_query(tf, "/external/active", 3, desc="End -> last item")

        tf.key(path="menu", name="ArrowRight")
        wait_query(tf, "/external/open", 1, desc="ArrowRight in menu -> switch to Edit")
        assert_eq(tf.query("/external/active"), 0, "switched menu highlights first")
        tf.key(path="menu", name="ArrowLeft")
        wait_query(tf, "/external/open", 0, desc="ArrowLeft in menu -> back to File")

        tf.key(path="menu", name="Escape")
        wait_query(tf, "/external/open", None, desc="Escape closes the menu")

        # Keyboard activation emits the command intent.
        tf.key(path="menu", name="ArrowDown")
        wait_query(tf, "/external/open", 0, desc="ArrowDown reopens File")
        tf.key(path="menu", name="Enter")
        wait_query(tf, "/external/open", None, desc="Enter activates + closes")
        _assert_command(tf, "0.0", "keyboard Enter")


if __name__ == "__main__":
    sys.exit(run_demo("R691 §5.16 §5.40 §5.50 — Menu widget first consumer", body))
