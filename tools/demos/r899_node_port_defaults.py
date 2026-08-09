#!/usr/bin/env python3
"""R899 §5.38 §5.40 — node-graph port default values (the "pin default").

Drives hello-node-editor over JSON-RPC. R898 made the ports typed sockets;
R899 gives each input port a typed literal **default value** — the value the
port carries when nothing is wired into it (the engine's "pin default value").
Typed by the port's PortType (a Float port -> a scalar, a Vector port -> a
colour), reusing the data-grid CellValue value substrate. An unconnected port
paints its default beside the pin; a wired port hides it (the wire supplies the
value); the value is retained while wired. AI-read/write + undoable.

  (A) boot — the seed graph's input defaults are typed + AI-readable; a wired
      port shows no default label (its value comes from the incoming edge).
  (B) an unconnected port paints its default; the typed AI write reads the
      outcome back (a colour takes a hex, a Float a scalar), wrong types and
      out-of-range ports are rejected.
  (C) wiring a port hides its default label but retains the value.
  (D) a default change is one undoable step (the AI write journals it).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
UNDO = "/node_undo/external"
WIN = (132 + 640, 420)
FIRST_DYN = 4  # ids 0..3 are seed nodes


def ncount(tf) -> int:
    return tf.query("/external/node_count")


def default_label(tf, tag: str):
    """The painted default label's text for port `tag`, or None if absent."""
    node = find_by_tag(tf.snapshot(source="paint", viewport=WIN), f"{G}#{tag}")
    if node is None:
        return None
    return node["children"][0]["content"]


def rejects(fn) -> bool:
    try:
        fn()
    except RpcError:
        return True
    return False


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: typed defaults, wired ports hide the label ────
        assert_eq(ncount(tf), 4, "boot: 4 seed nodes")
        # Multiply (node 2) input 0 is a Vector port -> a colour default.
        d = tf.query("/external/node.2.input_default.0")
        assert_eq(d["r"], 0x80, "Vector default is mid-grey (r)")
        assert_eq(d["g"], 0x80, "Vector default is mid-grey (g)")
        assert "hex" in d, "a colour default reads as a {hex,r,g,b,a} object"
        # Node 2's inputs are both wired (Texture/Color -> Multiply) -> no label.
        assert default_label(tf, "idefault_2_0") is None, "a wired input shows no default label"

        # ── (B) an open port paints its default; typed AI write ─────
        lerp = tf.invoke("/external/add_node", "Lerp")
        assert_eq(lerp, FIRST_DYN, "Lerp minted the first dynamic id")
        # Lerp's three inputs are unconnected -> each paints its default.
        l0 = wait_until(lambda: default_label(tf, f"idefault_{lerp}_0"), desc="open Vector port paints its default")
        assert_eq(l0, "#808080", "Vector port default paints as a hex colour")
        assert_eq(default_label(tf, f"idefault_{lerp}_2"), "0", "Float factor port default paints as 0")
        # back-compat: the R898 typed-port surface is intact.
        assert_eq(tf.query(f"/external/node.{lerp}.input_types"), "Vector,Vector,Float", "R898 types intact")
        assert_eq(tf.query(f"/external/node.{lerp}.inputs"), 3, "back-compat: arity still reads")
        # The Float factor default reads as a scalar; a typed write reads back.
        assert_eq(tf.query(f"/external/node.{lerp}.input_default.2"), 0.0, "Float default reads as a scalar")
        tf.intervene(f"/external/node.{lerp}.input_default.2", 0.5)
        assert_eq(tf.query(f"/external/node.{lerp}.input_default.2"), 0.5, "the typed Float write reads back")
        assert_eq(default_label(tf, f"idefault_{lerp}_2"), "0.5", "the paint reflects the written default")
        # A colour write takes a hex string and reads back the parsed channels.
        tf.intervene(f"/external/node.{lerp}.input_default.0", "#3366cc")
        d = tf.query(f"/external/node.{lerp}.input_default.0")
        assert_eq(d["r"], 0x33, "written colour r")
        assert_eq(d["b"], 0xCC, "written colour b")
        assert_eq(default_label(tf, f"idefault_{lerp}_0"), "#3366cc", "paint reflects the written colour")
        # Wrong value type for the port is rejected (no scalar into a colour,
        # no garbage into a Float), and out-of-range ports are unknown paths.
        assert rejects(lambda: tf.intervene(f"/external/node.{lerp}.input_default.0", 1.0)), "scalar into a colour rejected"
        assert rejects(lambda: tf.intervene(f"/external/node.{lerp}.input_default.2", "abc")), "garbage into a Float rejected"
        assert rejects(lambda: tf.intervene(f"/external/node.{lerp}.input_default.0", "#zzzz")), "malformed hex rejected"
        assert rejects(lambda: tf.query(f"/external/node.{lerp}.input_default.9")), "out-of-range port query is unknown"
        assert rejects(lambda: tf.intervene(f"/external/node.{lerp}.input_default.9", "#000000")), "out-of-range write is unknown"

        # ── (C) wiring a port hides the label but retains the value ──
        assert_eq(tf.invoke("/external/add_edge", f"0,0,{lerp},0"), True, "wire Texture -> Lerp input 0")
        wait_until(
            lambda: True if default_label(tf, f"idefault_{lerp}_0") is None else None,
            desc="the wired input's default label disappears",
        )
        assert default_label(tf, f"idefault_{lerp}_0") is None, "a wired port hides its default label"
        assert default_label(tf, f"idefault_{lerp}_1") is not None, "the still-open ports keep their labels"
        # The value is retained (wiring only hides the editor — the engine
        # model).
        d = tf.query(f"/external/node.{lerp}.input_default.0")
        assert_eq(d["r"], 0x33, "the wired port's default value is retained")

        # ── (D) a default change is one undoable step ───────────────
        tf.intervene(f"/external/node.{lerp}.input_default.1", "#112233")
        assert_eq(tf.query(f"/external/node.{lerp}.input_default.1")["r"], 0x11, "wrote a new default")
        assert_eq(tf.query(f"{UNDO}/can_undo"), True, "the default change is undoable")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the default change")
        assert_eq(tf.query(f"/external/node.{lerp}.input_default.1")["r"], 0x80, "undo restored the prior default")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo the default change")
        assert_eq(tf.query(f"/external/node.{lerp}.input_default.1")["r"], 0x11, "redo re-applied it")


if __name__ == "__main__":
    sys.exit(run_demo("R899 §5.38 §5.40 — node-graph port default values", body))
