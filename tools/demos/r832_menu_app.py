#!/usr/bin/env python3
"""R832 §5.40 §5.50 §5.38 — own-rendered application menu bar.

The R805 MenuBar substrate composed as application chrome: File / Edit /
View commands drive a real document model (content + dirty flag), exposed
for AI-first introspection through a `QueryOnlyIntrospect` node at `doc`.
This is the first menu-DRIVEN pinion application — the whole click -> menu
open -> item select -> command -> app-state-change loop is RPC-driven and
RPC-verified here.

Document state is read through the `doc` extra-external:
  /doc/external/{content_len, line_count, dirty, empty}

Menu structure (the `menu` primary external):
  File = [New, Open Sample, ---, Save]   (items 0,1,sep@2,3)
  Edit = [Append Line, Clear]            (items 0,1)
  View = [Show Status Bar(checkbox on), Uppercase(checkbox off)]

Verified:
  (A) boot taxonomy + empty document
  (B) File > Open Sample loads the sample (3 lines, clean)
  (C) Edit > Append Line grows the document and marks it modified
  (D) File > Save clears the dirty flag without changing content
  (E) Edit > Clear empties the document (modified)
  (F) File > New resets to a clean empty document
  (G) View > Uppercase transforms the rendered text (snapshot)
  (H) View > Show Status Bar toggles the status line (snapshot)
  (I) keyboard nav drives the menubar (focus + Arrow keys)

>= 30 assertions.
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
    wait_stderr,
    wait_until,
)

VIEWPORT = (560, 360)
MENU_TITLES = ["File", "Edit", "View"]

# doc introspect slots
LEN = "/doc/external/content_len"
LINES = "/doc/external/line_count"
DIRTY = "/doc/external/dirty"
EMPTY = "/doc/external/empty"


def _content_text(tf) -> list[str]:
    """All Text strings under the `content` container (body + status)."""
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, "content")
    out: list[str] = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text" and isinstance(n.get("content"), str):
                out.append(n["content"])
            for c in n.get("children") or []:
                walk(c)

    walk(node)
    return out


def _dropdown_painted(tf) -> bool:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, "menu_dropdown") is not None


def _open(tf, m: int) -> None:
    tf.click(path=f"menu#t{m}")
    # The External flips `open` immediately, but the dropdown (and its
    # `menu#i<i>` item tags) only enters the PAINT scene a frame later; the
    # item click resolves against the paint scene, so wait for it to land.
    wait_until(lambda: tf.query("/external/open") == m, timeout=4.0, interval=0.03,
               desc=f"menu {m} opens")
    wait_until(lambda: _dropdown_painted(tf), timeout=4.0, interval=0.05,
               desc=f"menu {m} dropdown painted")


def _activate(tf, i: int) -> None:
    """Click dropdown item `i`; the menu closes on activation."""
    tf.click(path=f"menu#i{i}")
    wait_until(lambda: tf.query("/external/open") is None, timeout=4.0, interval=0.03,
               desc=f"item {i} activates and closes the menu")


def _cmd(tf, m: int, i: int) -> None:
    _open(tf, m)
    _activate(tf, i)


def body() -> None:
    with RpcSubprocess("hello-menu-app", boot_grace=1.5) as tf:
        # ── (A) boot: menu bar + taxonomy + empty document ───────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, "menu") is not None, "menu bar present"
        assert_eq(tf.query("/external/item_kind.0.2"), "separator", "File 2 is a separator")
        assert_eq(tf.query("/external/item_kind.0.3"), "command", "File 3 (Save) is a command")
        assert_eq(tf.query("/external/item_kind.2.0"), "checkbox", "View 0 is a checkbox")
        assert_eq(tf.query("/external/checked.2.0"), True, "Show Status Bar boots checked")
        assert_eq(tf.query("/external/checked.2.1"), False, "Uppercase boots unchecked")
        assert_eq(tf.query(EMPTY), True, "document boots empty")
        assert_eq(tf.query(LEN), 0, "boot content_len 0")
        assert_eq(tf.query(LINES), 0, "boot line_count 0")
        assert_eq(tf.query(DIRTY), False, "boot not dirty")

        # ── (B) File > Open Sample ───────────────────────────────────
        _cmd(tf, 0, 1)
        wait_until(lambda: tf.query(LINES) == 3, timeout=4.0, interval=0.03,
                   desc="Open Sample loads 3 lines")
        assert_eq(tf.query(EMPTY), False, "document no longer empty")
        assert tf.query(LEN) > 0, "content_len positive after Open Sample"
        assert_eq(tf.query(DIRTY), False, "Open Sample loads a clean document")
        wait_stderr(
            tf,
            'menu.command payload=Text("0.1")',
            n=80,
            desc="Open Sample emits the command intent",
        )

        # ── (C) Edit > Append Line (x2): grows + marks modified ──────
        _cmd(tf, 1, 0)
        wait_until(lambda: tf.query(LINES) == 4, timeout=4.0, interval=0.03,
                   desc="Append Line -> 4 lines")
        assert_eq(tf.query(DIRTY), True, "Append Line marks the document modified")
        wait_stderr(
            tf,
            'menu.command payload=Text("1.0")',
            n=40,
            desc="Append Line emits the command intent",
        )
        _cmd(tf, 1, 0)
        wait_until(lambda: tf.query(LINES) == 5, timeout=4.0, interval=0.03,
                   desc="Append Line again -> 5 lines")

        # ── (D) File > Save: clears dirty, keeps content ─────────────
        len_before_save = tf.query(LEN)
        _cmd(tf, 0, 3)
        wait_until(lambda: tf.query(DIRTY) is False, timeout=4.0, interval=0.03,
                   desc="Save clears the dirty flag")
        assert_eq(tf.query(LEN), len_before_save, "Save does not change content")
        assert_eq(tf.query(LINES), 5, "Save keeps all 5 lines")

        # ── (E) Edit > Clear: empties (modified) ─────────────────────
        _cmd(tf, 1, 1)
        wait_until(lambda: tf.query(EMPTY) is True, timeout=4.0, interval=0.03,
                   desc="Clear empties the document")
        assert_eq(tf.query(LEN), 0, "Clear -> content_len 0")
        assert_eq(tf.query(DIRTY), True, "Clear marks the document modified")

        # ── (F) File > New: clean empty document ─────────────────────
        _cmd(tf, 0, 1)  # load sample again first
        wait_until(lambda: tf.query(EMPTY) is False, timeout=4.0, interval=0.03,
                   desc="reload sample before New")
        _cmd(tf, 0, 0)
        wait_until(lambda: tf.query(EMPTY) is True, timeout=4.0, interval=0.03,
                   desc="New empties the document")
        assert_eq(tf.query(DIRTY), False, "New resets to a clean document")

        # ── (G) View > Uppercase: transforms rendered text ───────────
        _cmd(tf, 0, 1)  # Open Sample so there is text to transform
        wait_until(lambda: tf.query(LINES) == 3, timeout=4.0, interval=0.03,
                   desc="sample loaded for the uppercase check")
        assert any("The quick" in t for t in _content_text(tf)), (
            "sample renders in mixed case before the toggle"
        )
        _cmd(tf, 2, 1)  # View > Uppercase ON
        assert_eq(tf.query("/external/checked.2.1"), True, "Uppercase toggled on")
        wait_until(lambda: any("THE QUICK" in t for t in _content_text(tf)),
                   timeout=4.0, interval=0.05, desc="content renders uppercased")
        assert not any("The quick" in t for t in _content_text(tf)), (
            "no mixed-case line remains while Uppercase is on"
        )
        _cmd(tf, 2, 1)  # Uppercase OFF
        assert_eq(tf.query("/external/checked.2.1"), False, "Uppercase toggled off")
        wait_until(lambda: any("The quick" in t for t in _content_text(tf)),
                   timeout=4.0, interval=0.05, desc="content renders mixed-case again")

        # ── (H) View > Show Status Bar: toggles the status line ──────
        assert any(t.startswith("lines:") for t in _content_text(tf)), (
            "status line present while Show Status Bar is checked"
        )
        _cmd(tf, 2, 0)  # Show Status Bar OFF
        assert_eq(tf.query("/external/checked.2.0"), False, "Show Status Bar toggled off")
        wait_until(lambda: not any(t.startswith("lines:") for t in _content_text(tf)),
                   timeout=4.0, interval=0.05, desc="status line hidden")
        _cmd(tf, 2, 0)  # back ON
        assert_eq(tf.query("/external/checked.2.0"), True, "Show Status Bar toggled back on")
        wait_until(lambda: any(t.startswith("lines:") for t in _content_text(tf)),
                   timeout=4.0, interval=0.05, desc="status line restored")

        # ── (I) keyboard nav drives the menubar ──────────────────────
        tf.request("focus/set", {"tag": "menu"})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == "menu",
            desc="menubar owns focus",
        )
        start = tf.query("/external/bar_focus")
        nxt = (start + 1) % len(MENU_TITLES)
        tf.key(path="menu", name="ArrowRight")
        wait_until(lambda: tf.query("/external/bar_focus") == nxt, timeout=4.0, interval=0.03,
                   desc=f"ArrowRight: bar_focus {start} -> {nxt}")
        tf.key(path="menu", name="ArrowDown")
        wait_until(lambda: tf.query("/external/open") == nxt, timeout=4.0, interval=0.03,
                   desc="ArrowDown opens the keyboard-focused menu")
        tf.key(path="menu", name="Escape")
        wait_until(lambda: tf.query("/external/open") is None, timeout=4.0, interval=0.03,
                   desc="Escape closes the menu")


if __name__ == "__main__":
    sys.exit(run_demo("hello-menu-app R832 §5.40 §5.50 application menu bar", body))
