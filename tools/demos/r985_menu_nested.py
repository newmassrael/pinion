#!/usr/bin/env python3
"""R985 §5.16 §5.38 §5.40 — cascading submenus.

Drives the `hello-menu-nested` binding's nested `MenuBarExternal` end-to-end
over JSON-RPC. The menu model:

  File = [New, Open Recent > [report.txt, notes.md, Older > [2024.log,
          2023.log]], --sep--, Save]
  Edit = [Undo, Redo]
  View = [Show Grid (checkbox, on), Appearance > [Light, Dark]]

Verification axes (>= 30 assertions):

  (A) introspection — `item_kind.<path>` / `item_count.<path>` address
      nested items by their descent path; a submenu reports `submenu`.
  (B) pointer cascade — clicking a submenu parent OPENS it (no command);
      `open_path` descends; clicking a nested leaf fires the full-path
      `menu.command` and closes the whole cascade.
  (C) keyboard cascade (WAI-ARIA §3.16) — Arrow Right opens a submenu,
      Arrow Left / Escape closes one level, Arrow Right on a leaf jumps to
      the next top-level menu.
  (D) paint — a submenu row paints a trailing chevron; each open level is a
      separate popup node.
  (E) a11y (`scene/access`) — the open submenu parent carries
      `aria-haspopup=menu` + `aria-expanded` and owns the nested `menu`.
  (F) intervene — `open_path` opens a submenu programmatically (no command),
      and rejects a non-submenu index.
  (G) a top-level checkbox still toggles (R805 behavior under nesting).
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

VIEWPORT = (560, 360)
CHEVRON = "▸"
CHECK_GLYPH = "✓"


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


def _popup_text(tf, tag: str) -> list[str]:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return _all_text(find_by_tag(snap, tag))


def _access(tf):
    return tf.request("scene/access").result


def _node(acc, tag):
    for n in acc["nodes"]:
        if n.get("tag") == tag:
            return n
    return None


def _open_title(tf, m: int) -> None:
    tf.click(path=f"menu#t{m}")
    wait_query(tf, "/external/open", m, desc=f"top menu {m} open")


def _focus_bar(tf) -> None:
    tf.request("focus/set", {"tag": "menu"})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == "menu",
        desc="menubar owns focus",
    )


def _key(tf, name: str) -> None:
    tf.key(path="menu", name=name)


def body() -> None:
    with RpcSubprocess("hello-menu-nested", boot_grace=1.5) as tf:
        # ── (A) nested structure introspection ──────────────────────
        assert_eq(tf.query("/external/menu_count"), 3, "File / Edit / View")
        assert_eq(tf.query("/external/item_kind.0.0"), "command", "File 0 = New")
        assert_eq(tf.query("/external/item_kind.0.1"), "submenu", "File 1 = Open Recent submenu")
        assert_eq(tf.query("/external/item_count.0.1"), 3, "Open Recent has 3 items")
        assert_eq(tf.query("/external/item_kind.0.1.2"), "submenu", "Open Recent 2 = Older submenu")
        assert_eq(tf.query("/external/item_count.0.1.2"), 2, "Older has 2 items")
        assert_eq(tf.query("/external/item_kind.0.1.2.0"), "command", "Older 0 = 2024.log")
        assert_eq(tf.query("/external/item_kind.0.2"), "separator", "File 2 = separator")
        assert_eq(tf.query("/external/item_kind.2.1"), "submenu", "View 1 = Appearance submenu")
        assert_eq(tf.query("/external/item_count.2.1"), 2, "Appearance has 2 items")
        assert_eq(tf.query("/external/item_kind.2.0"), "checkbox", "View 0 = Show Grid checkbox")
        assert_eq(tf.query("/external/checked.2.0"), True, "Show Grid boots checked")
        # A non-submenu has no nested item list (the path addresses no menu).
        no_list = False
        try:
            tf.query("/external/item_count.0.0")
        except Exception:  # noqa: BLE001 - RpcError surfaces the unknown path
            no_list = True
        assert no_list, "item_count of a command (non-submenu) is not a valid menu path"

        # ── (B) pointer cascade: open File > Open Recent > Older ─────
        _open_title(tf, 0)
        assert_eq(tf.query("/external/open_path"), "", "top dropdown, no submenu yet")
        # Click the Open Recent submenu parent -> it OPENS (no command).
        tf.click(path="menu#i1")
        wait_query(tf, "/external/open_path", "1", desc="Open Recent submenu opened")
        assert_eq(tf.query("/external/open"), 0, "the top menu stays open under a submenu")
        assert_eq(tf.query("/external/active_path"), "0.1.0", "submenu highlights its first item")
        # Descend again into Older.
        tf.click(path="menu#i1.2")
        wait_query(tf, "/external/open_path", "1.2", desc="Older submenu opened (2 levels deep)")
        assert_eq(tf.query("/external/active_path"), "0.1.2.0", "Older highlights its first item")
        # Activate a nested leaf -> full-path command + close the cascade.
        tf.click(path="menu#i1.2.0")
        wait_query(tf, "/external/open", None, desc="activating a nested leaf closes everything")
        wait_stderr(
            tf,
            'menu.command payload=Text("0.1.2.0")',
            n=80,
            desc="the command payload is the full absolute path",
        )

        # ── (C) keyboard cascade (WAI-ARIA §3.16) ───────────────────
        _focus_bar(tf)
        _open_title(tf, 0)
        _key(tf, "ArrowDown")
        wait_query(tf, "/external/active", 0, desc="ArrowDown -> New")
        _key(tf, "ArrowDown")
        wait_query(tf, "/external/active", 1, desc="ArrowDown -> Open Recent")
        # Arrow Right on a submenu parent descends.
        _key(tf, "ArrowRight")
        wait_query(tf, "/external/open_path", "1", desc="ArrowRight opens the submenu")
        wait_query(tf, "/external/active", 0, desc="submenu focuses its first item")
        _key(tf, "ArrowDown")
        _key(tf, "ArrowDown")
        wait_query(tf, "/external/active", 2, desc="ArrowDown -> Older (submenu parent)")
        _key(tf, "ArrowRight")
        wait_query(tf, "/external/open_path", "1.2", desc="ArrowRight descends a 2nd level")
        # Arrow Left closes one level, focus returns to the parent.
        _key(tf, "ArrowLeft")
        wait_query(tf, "/external/open_path", "1", desc="ArrowLeft closes the deepest level")
        wait_query(tf, "/external/active", 2, desc="focus returns to the Older parent")
        # Escape closes one more level.
        _key(tf, "Escape")
        wait_query(tf, "/external/open_path", "", desc="Escape collapses to the top dropdown")
        assert_eq(tf.query("/external/active"), 1, "focus returns to the Open Recent parent")
        _key(tf, "Escape")
        wait_query(tf, "/external/open", None, desc="a 2nd Escape closes the dropdown")

        # ── (D) Arrow Right on a LEAF jumps to the next top menu ─────
        _open_title(tf, 0)
        _key(tf, "ArrowDown")
        wait_query(tf, "/external/active", 0, desc="active = New (a leaf)")
        _key(tf, "ArrowRight")
        wait_query(tf, "/external/open", 1, desc="ArrowRight on a leaf -> next top menu (Edit)")
        assert_eq(tf.query("/external/open_path"), "", "the new menu opens at its top level")
        _key(tf, "Escape")
        wait_query(tf, "/external/open", None, desc="Escape closes Edit")

        # ── (E) paint: chevrons + nested popup ──────────────────────
        _open_title(tf, 0)
        tf.click(path="menu#i1")
        wait_query(tf, "/external/open_path", "1", desc="Open Recent open for paint check")
        top_text = _popup_text(tf, "menu_dropdown")
        assert "Open Recent" in top_text, f"top dropdown shows the submenu label; got {top_text!r}"
        assert CHEVRON in top_text, "the submenu parent row paints a trailing chevron"
        sub_text = _popup_text(tf, "menu_sub1")
        assert sub_text, "the nested submenu popup is painted as its own node"
        assert "report.txt" in sub_text and "Older" in sub_text, "the nested items render"
        assert CHEVRON in sub_text, "the nested Older submenu row also paints a chevron"

        # ── (F) a11y: aria-haspopup / aria-expanded / submenu ownership ─
        acc = _access(tf)
        parent = _node(acc, "menu#i1")
        assert parent is not None, "the Open Recent parent is in the access tree"
        assert_eq(parent["role"], "menuitem", "a submenu parent is a plain menuitem")
        assert_eq(parent["haspopup"], "menu", "aria-haspopup=menu is RPC-visible")
        assert_eq(parent["expanded"], True, "the open submenu parent is aria-expanded")
        assert "menu_sub1" in parent.get("children", []), "the parent owns the nested menu node"
        sub_menu = _node(acc, "menu_sub1")
        assert sub_menu is not None, "the nested menu node is in the access tree"
        assert_eq(sub_menu["role"], "menu", "the nested popup is a `menu`")
        assert_eq(sub_menu["name"], "Open Recent", "the nested menu is named for its parent")
        # The (closed) Older parent inside the submenu advertises a popup too.
        older = _node(acc, "menu#i1.2")
        assert older is not None, "the Older parent is in the access tree"
        assert_eq(older["haspopup"], "menu", "a collapsed submenu parent still has aria-haspopup")
        assert_eq(older["expanded"], False, "a collapsed submenu parent is aria-expanded=false")
        _key(tf, "Escape")
        _key(tf, "Escape")
        wait_query(tf, "/external/open", None, desc="close after the a11y probe")

        # ── (G) intervene open_path + the View checkbox under nesting ─
        _open_title(tf, 0)
        tf.intervene("/external/open_path", "1")
        wait_query(tf, "/external/open_path", "1", desc="intervene opened the submenu")
        # A non-submenu index is rejected.
        rejected = False
        try:
            tf.intervene("/external/open_path", "0")  # File 0 = New, a command
        except Exception:  # noqa: BLE001 - RpcError surfaces the OutOfRange reject
            rejected = True
        assert rejected, "intervene open_path on a non-submenu must be rejected"
        # Empty string collapses.
        tf.intervene("/external/open_path", "")
        wait_query(tf, "/external/open_path", "", desc="empty open_path collapses to the top")
        tf.click(path="menu#t0")  # close File
        wait_query(tf, "/external/open", None, desc="File closed")

        # View's top-level checkbox still toggles (R805 under nesting).
        _open_title(tf, 2)
        tf.click(path="menu#i0")  # Show Grid
        wait_query(tf, "/external/open", None, desc="a checkbox activation closes the menu")
        assert_eq(tf.query("/external/checked.2.0"), False, "Show Grid toggled off")
        wait_stderr(
            tf,
            'menu.command payload=Text("2.0")',
            n=80,
            desc="the checkbox activation emits its command",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R985 §5.40 — cascading submenus", body))
