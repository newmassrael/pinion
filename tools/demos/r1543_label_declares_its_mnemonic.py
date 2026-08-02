#!/usr/bin/env python3
"""R1543 §5.39 §5.40 §5.12 — a label declares its mnemonic (Qt `&File`).

Before this round pinion had no mnemonics at all. The `menu` module deferred
"accelerator / mnemonic keys" as an axis awaiting a consumer, and what an
absent extension point actually produces showed up instead: 96 bindings
hand-rolled a `keybinding` map of BARE characters with no relation to any
painted label — no underline, invisible to assistive technology, no conflict
check, and colliding with text input for want of a modifier.

R1543 makes the mnemonic ONE declaration on the label, from which three facts
are derived and none of them is a second source:

  * the **ink** — a `StyleRun` underlining exactly the marked character, which
    both painters already knew how to draw (R713 styled runs, R1540 underline
    forms), so the capability reached the GUI and the terminal with no
    per-backend paint code;
  * the **binding** — `scene_mnemonics` over the PAINT scene, which is why
    what Alt+F hits is by construction what a sighted user sees underlined;
  * the **announcement** — `accesskey` (UIA `AccessKey` / HTML `accesskey`),
    stamped at the a11y assembler chokepoint.

What this demo asserts, over the wire, against a real menubar application:

  * `scene/mnemonics` publishes the whole accelerator map — key, platform
    spelling, target tag, display label, marked byte offset, conflicts. Qt
    keeps this in `QShortcutMap`, a private header: a Qt application cannot
    enumerate its own accelerators, let alone an external driver.
  * the published label is the DISPLAY string — the `&` never reaches pixels
    or the AT — while the index still locates the marked character in it;
  * `scene/access` carries the same key as `accesskey` on the same tag, so
    what a screen reader is told and what the map says are one declaration;
  * Alt+F opens the File menu — the accelerator reaching a COMPOSITE paint
    tag (`menu#t0`) through the same R51.42 wire a click uses;
  * the map GROWS while a dropdown is open and shrinks when it closes, with
    no registration anywhere: an item's mnemonic exists exactly while its
    label is painted, which is AccessKit's stated semantics for menu access
    keys and Qt's behaviour, and here it falls out of deriving from paint;
  * Alt+S then activates Save, emitting the same `menu.command` intent a
    click emits — the mnemonic takes the widget's existing path, not a
    parallel one;
  * an UNDECLARED Alt+key falls through and changes nothing, and the same
    character WITHOUT Alt does not activate — the two negative controls that
    separate "the mnemonic fired" from "any keypress opened a menu".

ZERO-FLAKE: bounded `wait_until` polling (never a fixed sleep). >=30
assertions.

Run from the workspace root:
    cargo build -p hello-menu-app --release
    python3 tools/demos/r1543_label_declares_its_mnemonic.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_stderr,
    wait_until,
)

VIEWPORT = (900, 620)
LINES = "/doc/external/line_count"
DIRTY = "/doc/external/dirty"

# The three menubar titles, as authored (`"&File"`) and as they must appear.
TITLES = [("F", "File", "menu#t0"), ("E", "Edit", "menu#t1"), ("V", "View", "menu#t2")]

# The exact published shape of one map entry. A response an agent is told to
# rely on should not be able to gain or lose a key unnoticed.
ENTRY_KEYS = {"key", "accel", "target", "label", "index", "ambiguous"}


def mnemonics(tf) -> list[dict[str, Any]]:
    return tf.request("scene/mnemonics").result["mnemonics"]


def entry_for(rows: list[dict[str, Any]], key: str) -> Optional[dict[str, Any]]:
    for row in rows:
        if row["key"] == key:
            return row
    return None


def press(tf, char: str, *, alt: bool = True) -> None:
    """Type `char`, with or without Alt held.

    The modifier is set out of band exactly as winit delivers it
    (`ModifiersChanged` is a separate event from the key), which is the same
    discipline every other modifier-bearing shortcut in this repo follows.
    """
    tf.modifiers(alt=alt)
    tf.key(at=(5.0, 5.0), name=char)
    tf.modifiers()


def dropdown_painted(tf) -> bool:
    return find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), "menu_dropdown") is not None


def body() -> None:
    with RpcSubprocess("hello-menu-app", boot_grace=1.5) as tf:
        # ── (A) the map is published, complete, and conflict-free ──────────
        wait_until(
            lambda: len(mnemonics(tf)) == 3,
            timeout=4.0,
            interval=0.03,
            desc="the menubar's three titles publish their mnemonics",
        )
        rows = mnemonics(tf)
        assert_eq(len(rows), 3, "three titles, three accelerators")
        assert_eq(set(rows[0].keys()), ENTRY_KEYS, "an entry's exact key set")
        for pos, (key, label, target) in enumerate(TITLES):
            row = rows[pos]
            assert_eq(row["key"], key, f"{label}: the marked character")
            assert_eq(row["accel"], f"Alt+{key}", f"{label}: platform spelling")
            assert_eq(row["target"], target, f"{label}: the tag it activates")
            assert_eq(row["label"], label, f"{label}: the `&` is not in the label")
            assert_eq(row["index"], 0, f"{label}: the mark is at byte 0")
            assert_eq(row["ambiguous"], False, f"{label}: nothing else claims {key}")
        assert_eq(
            [r["key"] for r in rows],
            ["F", "E", "V"],
            "paint order — which is also the order an ambiguous key would cycle",
        )

        # ── (B) the ampersand never reaches pixels ─────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        for _key, label, target in TITLES:
            title = find_by_tag(snap, target)
            assert title is not None, f"{label} title is painted"
            painted = [
                n["content"]
                for n in (title.get("children") or [])
                if n.get("type") == "Text"
            ]
            assert_eq(painted, [label], f"{label}: painted without its marker")

        # ── (C) the AT is told the same key, on the same node ──────────────
        access = tf.request("scene/access").result
        for key, label, target in TITLES:
            node = access_node_by_tag(access, target)
            assert node is not None, f"{label} has an access node"
            assert_eq(node.get("accesskey"), f"Alt+{key}", f"{label}: announced key")
        assert_eq(
            access_node_by_tag(access, "menu").get("accesskey"),
            None,
            "the menubar itself declares none — the stamp lands on the target, "
            "not on everything nearby",
        )

        # ── (D) Alt+F opens File: the accelerator reaches a composite tag ──
        press(tf, "f")
        wait_until(
            lambda: tf.query("/external/open") == 0,
            timeout=4.0,
            interval=0.03,
            desc="Alt+F opens the File menu",
        )
        wait_until(lambda: dropdown_painted(tf), timeout=4.0, interval=0.05,
                   desc="the File dropdown paints")

        # ── (E) the map grows with the open dropdown, unregistered ─────────
        rows_open = mnemonics(tf)
        assert len(rows_open) > 3, "the open dropdown's items publish theirs too"
        for key, target in (("N", "menu#i0"), ("O", "menu#i1"), ("S", "menu#i3")):
            row = entry_for(rows_open, key)
            assert row is not None, f"item mnemonic {key} is published while open"
            assert_eq(row["target"], target, f"{key} addresses its own item")
        assert_eq(
            entry_for(rows_open, "A"),
            None,
            "the Edit menu's items are NOT bound: their labels are not painted, "
            "and the map is derived from what is painted",
        )
        assert_eq(
            len({r["key"] for r in rows_open}),
            len(rows_open),
            "no key is contested while File is open",
        )

        # ── (F) Alt+S activates Save through the widget's own path ─────────
        press(tf, "s")
        wait_until(
            lambda: tf.query("/external/open") is None,
            timeout=4.0,
            interval=0.03,
            desc="Alt+S activates Save and closes the menu",
        )
        wait_stderr(
            tf,
            'menu.command payload=Text("0.3")',
            n=120,
            desc="the mnemonic emits the SAME command intent a click emits",
        )
        assert_eq(tf.query(DIRTY), False, "Save left the document clean")

        # ── (G) the map shrinks back when the dropdown closes ──────────────
        wait_until(
            lambda: len(mnemonics(tf)) == 3,
            timeout=4.0,
            interval=0.05,
            desc="the item accelerators unbind with their dropdown",
        )
        assert_eq(entry_for(mnemonics(tf), "N"), None, "no stale registration")

        # ── (H) a second title, and Alt+A inside it ────────────────────────
        press(tf, "e")
        wait_until(
            lambda: tf.query("/external/open") == 1,
            timeout=4.0,
            interval=0.03,
            desc="Alt+E opens the Edit menu",
        )
        lines_before = tf.query(LINES)
        press(tf, "a")
        wait_until(
            lambda: tf.query(LINES) == lines_before + 1,
            timeout=4.0,
            interval=0.03,
            desc="Alt+A runs Append Line",
        )
        assert_eq(tf.query("/external/open"), None, "and the menu closed")

        # ── (I) negative control: an undeclared accelerator ────────────────
        lines_now = tf.query(LINES)
        press(tf, "z")
        assert_eq(tf.query("/external/open"), None, "Alt+Z opens nothing")
        assert_eq(tf.query(LINES), lines_now, "and runs nothing")

        # ── (J) negative control: the same character without Alt ───────────
        # The one that separates "the mnemonic fired" from "a keypress opened
        # a menu". Without the modifier the character is text, not a command.
        press(tf, "f", alt=False)
        assert_eq(
            tf.query("/external/open"),
            None,
            "a bare `f` is not an accelerator — the modifier is the gate",
        )
        assert_eq(tf.query(LINES), lines_now, "and it ran nothing either")

        # ── (K) the published index locates the mark in the published label ─
        row = entry_for(mnemonics(tf), "V")
        assert row is not None, "View is still bound"
        label, index = row["label"], row["index"]
        assert_eq(label[index], "V", "index addresses the label an agent was given")
        assert_eq(
            tf.query("/external/open"),
            None,
            "reading the map changed nothing",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1543 §5.39 a label declares its mnemonic", body))
