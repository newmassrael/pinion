#!/usr/bin/env python3
"""R901 §5.38 §5.40 — node-graph inline port-default editor.

Drives hello-node-editor over JSON-RPC. R899 gave each input port a typed
literal default value but deferred the interactive keyboard editor; R901 lands
it by generalising the R878 single-purpose rename editor into ONE shared inline
field keyed by an `EditTarget` (`Title` | `PortDefault`). Double-click a node
card edits its title (R878); double-click a pin's default label edits that
port's default. Both reuse the same field, focus, blur-commit, keymap, and
a11y machinery — the keystroke gate is the *target's* CellKind (a Float pin
takes a number, a Vector pin a `#RRGGBB[AA]` hex). A commit journals an
undoable `SetPortDefaultCmd` through the same `apply_set_default` SSOT the AI
`intervene` write drives.

  (A) boot — no edit in flight (`query editing` / `renaming` both Null).
  (B) double-click a pin's default opens the field seeded from the value;
      `query editing` reports the port-default target, `renaming` stays Null
      (the honest degenerate projection — a port edit is not a rename).
  (C) typing a new value + Enter commits through `apply_set_default` (the
      paint + query update); it is one undoable step.
  (D) the keystroke gate is the port's type (a letter never reaches a Float
      pin); Escape cancels without touching the value.
  (E) a Vector pin edits as a `#RRGGBB[AA]` hex (the same field, Color gate).
  (F) `invoke begin_edit_default` / `query editing` — the RPC pair (unknown
      node / out-of-range port rejected).
  (G) migration: opening a title rename commits the in-flight port default.
  (H) a click-away commits (the R793 blur intent).
  (I) the editor works on a zoomed canvas (the R877 viewport).

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
EDIT = "node_edit"  # R901 — the shared inline edit field's tag
UNDO = "/node_undo/external"
VIEWPORT = (772, 420)
FIRST_DYN = 4  # ids 0..3 are seed nodes


def editing(tf):
    return tf.query("/external/editing")


def renaming(tf):
    return tf.query("/external/renaming")


def pin_default(tf, node_id: int, port: int):
    return tf.query(f"/external/node.{node_id}.input_default.{port}")


def editor_text(tf):
    return tf.query(f"/{EDIT}/external/text")


def default_label(tf, tag: str):
    """The painted default label's text for port `tag`, or None if absent."""
    node = find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), f"{G}#{tag}")
    if node is None:
        return None
    return node["children"][0]["content"]


def field_painted(tf) -> bool:
    return EDIT in abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def rejects(fn) -> bool:
    try:
        fn()
    except RpcError:
        return True
    return False


def retype(tf, new_text: str) -> None:
    """Erase the seeded value (caret parks at the end) and type a new one."""
    for _ in range(len(editor_text(tf))):
        tf.key(path=EDIT, name="Backspace")
    tf.text(new_text, path=EDIT)


def open_pin(tf, node_id: int, port: int) -> None:
    tf.double_click(path=f"{G}#idefault_{node_id}_{port}")
    wait_until(
        lambda: editing(tf) == {"kind": "port_default", "node": node_id, "port": port},
        timeout=4.0,
        interval=0.03,
        desc=f"dblclick opens the editor on node {node_id} port {port}",
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — idle ─────────────────────────────────────────
        assert_eq(editing(tf), None, "boot: no edit in flight")
        assert_eq(renaming(tf), None, "boot: no rename in flight")
        assert not field_painted(tf), "the shared field is unpainted while idle"
        # A fresh Lerp (inputs [Vector, Vector, Float]) has unwired pins, so each
        # paints its editable default.
        lerp = tf.invoke("/external/add_node", "Lerp")
        assert_eq(lerp, FIRST_DYN, "Lerp minted the first dynamic id")
        # Park the Lerp in clear canvas space (the seed nodes sit at y<=210), so
        # a geometric `double_click(path=...)` on its pins (or on a seed card,
        # for the migration step) lands without an overlapping card on top.
        tf.intervene(f"/external/node.{lerp}.x", 300)
        tf.intervene(f"/external/node.{lerp}.y", 260)
        assert_eq(default_label(tf, f"idefault_{lerp}_2"), "0", "the Float pin default paints as 0")

        # ── (B) double-click opens the seeded editor ────────────────
        open_pin(tf, lerp, 2)
        wait_until(lambda: field_painted(tf), timeout=4.0, interval=0.03,
                   desc="the shared field paints over the pin default")
        assert_eq(editor_text(tf), "0", "editor seeded with the current default")
        assert_eq(tf.request("focus/get").result.get("focused"), EDIT,
                  "the field owns keyboard focus")
        # `editing` is the port-default target; `renaming` is the honest Null.
        assert_eq(editing(tf), {"kind": "port_default", "node": lerp, "port": 2},
                  "query editing reports the port-default target")
        assert_eq(renaming(tf), None, "a port-default edit is not a rename")

        # ── (C) typing + Enter commits (one undoable step) ──────────
        retype(tf, "0.75")
        wait_until(lambda: editor_text(tf) == "0.75", timeout=4.0, interval=0.03,
                   desc="keystrokes reach the shared field")
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03,
                   desc="Enter leaves edit mode")
        assert_eq(pin_default(tf, lerp, 2), 0.75, "the commit applied the typed default")
        assert not field_painted(tf), "the field unpaints after the commit"
        assert_eq(tf.request("focus/get").result.get("focused"), G, "focus returns to the canvas")
        assert_eq(default_label(tf, f"idefault_{lerp}_2"), "0.75", "the paint reflects the new default")
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Set port default", "journaled undoably")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the default change")
        assert_eq(pin_default(tf, lerp, 2), 0.0, "undo restored the prior default")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo the default change")
        assert_eq(pin_default(tf, lerp, 2), 0.75, "redo re-applied it")

        # ── (D) the keystroke gate is the port's type; Escape cancels ─
        open_pin(tf, lerp, 2)  # a Float pin
        tf.key(path=EDIT, name="x")  # a letter is gated out of a Float field
        assert_eq(editor_text(tf), "0.75", "a non-numeric keystroke never reaches a Float pin")
        retype(tf, "9.9")
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03, desc="Escape leaves edit mode")
        assert_eq(pin_default(tf, lerp, 2), 0.75, "a cancel never touches the default")

        # ── (E) a Vector pin edits as a hex through the same field ──
        assert_eq(default_label(tf, f"idefault_{lerp}_0"), "#808080", "Vector pin default paints as hex")
        open_pin(tf, lerp, 0)
        assert_eq(editor_text(tf), "#808080", "seeded with the grey default's hex")
        retype(tf, "#3366cc")
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03, desc="hex commit leaves edit mode")
        d = pin_default(tf, lerp, 0)
        assert_eq(d["r"], 0x33, "the typed hex parsed into the colour default (r)")
        assert_eq(d["b"], 0xCC, "the typed hex parsed into the colour default (b)")
        assert_eq(default_label(tf, f"idefault_{lerp}_0"), "#3366cc", "paint reflects the written colour")

        # ── (F) the RPC pair: begin_edit_default / editing ──────────
        assert_eq(tf.invoke("/external/begin_edit_default", f"{lerp}.1"), True, "begin_edit_default opens a pin")
        assert_eq(editing(tf), {"kind": "port_default", "node": lerp, "port": 1},
                  "query editing reports the RPC-opened target")
        assert_eq(renaming(tf), None, "the port-default edit is not a rename")
        assert_eq(tf.invoke("/external/begin_edit_default", "99.0"), False, "an unknown node is rejected")
        assert_eq(tf.invoke("/external/begin_edit_default", f"{lerp}.9"), False,
                  "an out-of-range port is rejected")
        assert rejects(lambda: tf.invoke("/external/begin_edit_default", 7)), "a non-Text arg is a type mismatch"
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03, desc="cleanup: Escape")

        # ── (G) migration: a title rename commits the in-flight pin ─
        open_pin(tf, lerp, 2)
        retype(tf, "2.5")
        wait_until(lambda: editor_text(tf) == "2.5", timeout=4.0, interval=0.03,
                   desc="typed the migration candidate")
        tf.double_click(path=f"{G}#node_2")  # opening a title rename migrates
        wait_until(lambda: editing(tf) == {"kind": "title", "node": 2}, timeout=4.0, interval=0.03,
                   desc="the editor migrated to the title target")
        assert_eq(pin_default(tf, lerp, 2), 2.5, "opening the title editor committed the in-flight pin default")
        assert_eq(editor_text(tf), "Multiply", "the editor reseeded from the title target")
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03, desc="cleanup: Escape")

        # ── (H) a click-away commits (R793 blur intent) ─────────────
        open_pin(tf, lerp, 1)
        retype(tf, "#112233")
        wait_until(lambda: editor_text(tf) == "#112233", timeout=4.0, interval=0.03,
                   desc="typed the blur-commit candidate")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        gx, gy, gw, gh = abs_rects_of(snap)[G]
        tf.click(at=(gx + gw - 12, gy + gh - 12))  # empty canvas -> blur -> commit
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03,
                   desc="the blur committed and left edit mode")
        assert_eq(pin_default(tf, lerp, 1)["r"], 0x11, "the click-away committed the typed colour")

        # ── (I) the editor works on a zoomed canvas (R877) ──────────
        tf.intervene("/external/viewport.zoom", 1.5)
        assert_eq(tf.query("/external/viewport.zoom"), 1.5, "canvas at 150%")
        tf.invoke("/external/frame_all", None)
        open_pin(tf, lerp, 2)
        assert field_painted(tf), "the field paints inside the zoomed pin row"
        retype(tf, "3.5")
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03, desc="zoomed commit leaves edit mode")
        assert_eq(pin_default(tf, lerp, 2), 3.5, "a port-default edit at 150% commits identically")


if __name__ == "__main__":
    sys.exit(run_demo("R901 §5.38 §5.40 — node-graph inline port-default editor", body))
