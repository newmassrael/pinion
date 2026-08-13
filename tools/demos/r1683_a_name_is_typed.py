#!/usr/bin/env python3
"""R1683 §5.20 §5.22 — the analyser's node screen learns to be typed into.

Drives `hello-node-lab` over JSON-RPC. Until this round the screen had no text
entry ANYWHERE, and that one absence answered for every operation needing a
value typed: the rename had a verb and no gesture, "add a field by typing its
key" had neither, the settings form's text rows resolved a press to a named
target and dropped it, and the launch gate could be closed by an agent and not
by a person. Registered as an AXIS rather than four omissions, because a round
that bolted a name box on would have answered one of them.

So one field — the framework's own `TextEditState` behind the shared text-field
painter, with the lifted edit keymap as its fourth call site — with a TARGET.
The same box renames a card and adds a configuration path, which is what makes
it a substrate arriving rather than a widget appearing.

What this demo proves that the in-process gates cannot: the KEYSTROKE path. A
character reaches the buffer through the field's own external, and that external
is mounted by the shell — so the sweep next door, which builds a scene by
calling the view directly, has no external to forward to and says so rather than
pretending. Here there is a real shell.

  (A) boot — the field is shut, and the wire says so.
  (B) open it on the name with a real pointer press; it holds the current name,
      selected whole so the first keystroke replaces.
  (C) TYPE, one key at a time, through the shell's key path — and the card is
      not renamed until it is applied.
  (D) apply with the seat; the card answers to the new name and keeps its
      identity (its links are the links it had).
  (E) a name another card holds is refused, and the field stays open holding
      the text so it can be edited rather than retyped.
  (F) Escape closes and changes nothing.
  (G) the same field, other target: type a configuration path the catalogue
      does not offer and see the form grow a row.

>= 30 assertions.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    assert_router_press_moves,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"
VIEWPORT = (1440, 900)


def q(tf, path):
    return tf.query(f"{EXT}/{path}")


def editing(tf) -> dict:
    return json.loads(q(tf, "editing"))


def rects(tf) -> dict:
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def press(tf, tag):
    box = rects(tf)[tag]
    tf.click(at=(box[0] + box[2] // 2, box[1] + box[3] // 2))


def type_keys(tf, text):
    """One key event per character, the way a keyboard produces them.

    Aimed at the field itself, because `scene/key` resolves a cursor target and
    the dispatcher routes from there — the same place a person's caret is.
    """
    for ch in text:
        tf.key(path="lab.edit", name=ch)


def refused(tf, verb, args) -> str:
    try:
        tf.invoke(f"{EXT}/{verb}", args)
    except RpcError as why:
        return str(why)
    raise AssertionError(f"{verb} {args!r} was accepted and had to be refused")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot ────────────────────────────────────────────────
        shut = editing(tf)
        assert_eq(shut["target"], None, "the field opens shut")
        assert_eq(shut["text"], "", "and empty")
        painted = rects(tf)
        assert "lab.inspector.rename" in painted, "the seat that opens it is there"
        assert "lab.inspector.addkey" in painted, "and the seat for the other target"
        assert "lab.inspector.name" in painted, "with a placeholder where the box will be"

        spec = json.loads(q(tf, "spec"))
        ops = {op["name"]: op for op in spec["operations"]}
        for name in ("rename a node", "add a field by typing its key"):
            assert_eq(ops[name]["gesture"], True, f"★ {name!r} has a way in for a person now")
            assert_eq(ops[name]["absent"], False, f"and {name!r} is answered")

        # ── (B) open it on the name, with a pointer ─────────────────
        tf.invoke(f"{EXT}/select", "P-03")
        press(tf, "lab.inspector.rename")
        open_now = editing(tf)
        assert_eq(open_now["target"], "name", "★ the seat opened the field on the name")
        assert_eq(open_now["text"], "P-03", "seeded with what the card is called")
        assert "lab.inspector.name" not in rects(tf), "the placeholder gave way to the field"

        # ── (C) type, and see that typing alone renames nothing ─────
        before = q(tf, "nodes")
        type_keys(tf, "edge-01")
        assert_eq(
            editing(tf)["text"],
            "edge-01",
            "★★ the keystrokes reached the buffer — this is the path the "
            "in-process sweep cannot drive, because the field's external is "
            "the shell's to mount",
        )
        assert_eq(q(tf, "nodes"), before, "★ and typing has renamed nothing yet")
        assert_eq(q(tf, "selected"), "P-03", "the card is still called what it was")

        # ── (D) apply ───────────────────────────────────────────────
        links_before = json.loads(q(tf, "links"))
        assert_router_press_moves(
            tf, "lab.inspector.rename", lambda: q(tf, "nodes"), "the card is renamed"
        )
        assert "edge-01" in q(tf, "nodes").split(","), "★ applied with the same seat"
        assert "P-03" not in q(tf, "nodes").split(","), "and not the old name"
        assert_eq(editing(tf)["target"], None, "the field shut behind it")
        assert_eq(editing(tf)["text"], "", "and let go of the text")
        assert_eq(
            [link["id"] for link in json.loads(q(tf, "links"))],
            [link["id"] for link in links_before],
            "★★ and it is the same card — no link was re-minted",
        )
        assert_eq(q(tf, "selected"), "edge-01", "the selection came with it")

        # ── (E) a taken name is refused, and the text survives ──────
        press(tf, "lab.inspector.rename")
        assert_eq(editing(tf)["text"], "edge-01", "it opens on the current name")
        tf.invoke(f"{EXT}/type", "P-01")
        why = refused(tf, "apply", "")
        assert "already called" in why, f"★ the model refuses, and says who holds it: {why}"
        assert_eq(
            editing(tf)["target"],
            "name",
            "★★ and the field is STILL OPEN — a person whose name was rejected "
            "wants to edit it, not to type it again",
        )
        assert_eq(editing(tf)["text"], "P-01", "holding what they typed")
        assert "edge-01" in q(tf, "nodes").split(","), "the card kept its name"

        # ── (F) Escape closes and changes nothing ───────────────────
        tf.key(path="lab.edit", name="Escape")
        assert_eq(editing(tf)["target"], None, "★ Escape shuts it")
        assert "edge-01" in q(tf, "nodes").split(","), "and renamed nothing"

        # ── (G) the same field, the other target ────────────────────
        keys_before = [f["key"] for f in json.loads(q(tf, "form"))]
        assert "transport.unicast.lowlatency" not in keys_before, "not held, and not offered"
        assert all(
            chip != "transport.unicast.lowlatency" for chip in spec.get("addable", [])
        ), "the catalogue does not offer it, which is the point of typing one"

        press(tf, "lab.inspector.addkey")
        assert_eq(editing(tf)["target"], "key", "★ the same field, a different target")
        assert_eq(editing(tf)["text"], "", "and this one opens empty — there is no key yet")
        type_keys(tf, "transport.unicast.lowlatency")
        press(tf, "lab.inspector.rename")
        keys_after = [f["key"] for f in json.loads(q(tf, "form"))]
        assert "transport.unicast.lowlatency" in keys_after, (
            "★★ a path the catalogue never offered is on the form, typed"
        )
        assert_eq(
            len(keys_after), len(keys_before) + 1, "exactly one row appeared"
        )
        assert_eq(editing(tf)["target"], None, "and the field shut")

        # Twice is refused rather than silently duplicated.
        press(tf, "lab.inspector.addkey")
        tf.invoke(f"{EXT}/type", "transport.unicast.lowlatency")
        why = refused(tf, "apply", "")
        assert "already holds" in why, f"a key it already has is refused: {why}"
        assert_eq(
            len([f for f in json.loads(q(tf, "form"))
                 if f["key"] == "transport.unicast.lowlatency"]),
            1,
            "★ and there is still exactly one such row",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1683 §5.22 — the screen learns to be typed into", body))
