#!/usr/bin/env python3
"""R898 §5.38 §5.40 — node-graph typed ports (the blueprint / material-graph
foundation).

Drives hello-node-editor over JSON-RPC. R838-R897 made the graph movable /
connectable / authorable, but its ports were bare arity *counts* — `add_edge`
checked only "does the port index exist", so a scalar could wire into anything.
R898 makes the ports **typed sockets** (`PortType`): an edge connects an output
to an input only when the output's type is *assignable* to the input's. This is
the type lattice the self-hosted blueprint / material-graph editor needs to
reject ill-typed wires the way the engine / the DCC do.

  (A) boot — the seed material graph carries typed ports, AI-readable via
      `query node.<id>.{input_types,output_types}`; the pre-R898 arity
      (`inputs` / `outputs` counts) reads identically (back-compat).
  (B) type-checked connect — the appended typed sources/ops (`Scalar` Float
      source, `Lerp` with a Float factor input) exercise the lattice: exact
      matches and a `Float`->`Vector` scalar broadcast are accepted, a
      `Vector`->`Float` narrowing is rejected, the graph unchanged.
  (C) the typed-port lists are read-only (ports are the node kind's, edited
      only by add/remove edges).
  (D) color-coded pins — each port paints in its type's signature colour
      (the engine/the DCC convention), verified via scene-as-data fill (no
      pixels needed — the §2 #7 introspection axis).
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
# PALETTE_W (132) + WIN_W (640), WIN_H (420).
WIN = (132 + 640, 420)
# Ids 0..3 are the seed nodes; the first minted id is 4.
FIRST_DYN = 4

# R898 — the PortType signature colours (FLOAT_PORT_COLOR / VECTOR_PORT_COLOR).
FLOAT_GREEN = (0x7C, 0xD0, 0x6F)
VECTOR_GOLD = (0xE0, 0xB0, 0x3A)


def ncount(tf) -> int:
    return tf.query("/external/node_count")


def ecount(tf) -> int:
    return tf.query("/external/edge_count")


def assert_color(actual: dict, expected: tuple[int, int, int], label: str) -> None:
    got = (actual["r"], actual["g"], actual["b"])
    assert got == expected, f"{label}: color {got} != expected {expected}"


def port_fill(tf, tag: str) -> dict:
    """The scene-as-data fill colour of the port box `tag` (waits for paint)."""

    def look():
        node = find_by_tag(tf.snapshot(source="paint", viewport=WIN), tag)
        return node if node is not None else None

    node = wait_until(look, desc=f"port {tag} painted")
    return node["style"]["fill"]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: the seed graph carries typed ports ────────────
        assert_eq(ncount(tf), 4, "boot: 4 seed nodes")
        assert_eq(ecount(tf), 3, "boot: 3 seed edges")
        # The material graph's data type is Vector (colour / vec3).
        assert_eq(tf.query("/external/node.0.output_types"), "Vector", "Texture outputs a Vector")
        assert_eq(tf.query("/external/node.0.input_types"), "", "Texture is a source (no inputs)")
        assert_eq(tf.query("/external/node.1.output_types"), "Vector", "Color outputs a Vector")
        assert_eq(tf.query("/external/node.2.input_types"), "Vector,Vector", "Multiply takes 2 Vectors")
        assert_eq(tf.query("/external/node.2.output_types"), "Vector", "Multiply outputs a Vector")
        assert_eq(tf.query("/external/node.3.input_types"), "Vector", "Output takes a Vector")
        assert_eq(tf.query("/external/node.3.output_types"), "", "Output is a sink (no outputs)")
        # Back-compat: the pre-R898 arity reads are byte-identical.
        assert_eq(tf.query("/external/node.2.inputs"), 2, "back-compat: Multiply arity is still 2 in")
        assert_eq(tf.query("/external/node.2.outputs"), 1, "back-compat: Multiply arity is still 1 out")
        assert_eq(tf.query("/external/node.3.inputs"), 1, "back-compat: Output arity is still 1 in")

        # ── (B) typed sources/ops + type-checked connect ────────────
        scalar = tf.invoke("/external/add_node", "Scalar")
        lerp = tf.invoke("/external/add_node", "Lerp")
        assert_eq(scalar, FIRST_DYN, "Scalar mints the first dynamic id")
        assert_eq(lerp, FIRST_DYN + 1, "Lerp mints the next id")
        assert_eq(tf.query(f"/external/node.{scalar}.output_types"), "Float", "Scalar outputs a Float")
        assert_eq(tf.query(f"/external/node.{scalar}.input_types"), "", "Scalar is a source")
        assert_eq(
            tf.query(f"/external/node.{lerp}.input_types"),
            "Vector,Vector,Float",
            "Lerp takes two Vectors and a Float factor",
        )
        assert_eq(tf.query(f"/external/node.{scalar}.inputs"), 0, "back-compat: Scalar arity 0 in")
        assert_eq(tf.query(f"/external/node.{lerp}.inputs"), 3, "back-compat: Lerp arity 3 in")

        base = ecount(tf)  # 3 seed edges; adding nodes added none.
        assert_eq(base, 3, "adding nodes adds no edges")
        # Float -> Float (Lerp's factor input): exact match, accepted.
        assert_eq(
            tf.invoke("/external/add_edge", f"{scalar},0,{lerp},2"),
            True,
            "Float -> Float (exact) is accepted",
        )
        assert_eq(ecount(tf), base + 1, "the exact-match wire landed")
        # Float -> Vector (Lerp's colour input): scalar broadcast, accepted.
        assert_eq(
            tf.invoke("/external/add_edge", f"{scalar},0,{lerp},0"),
            True,
            "Float -> Vector (scalar broadcast) is accepted",
        )
        assert_eq(ecount(tf), base + 2, "the broadcast wire landed")
        # Vector -> Float (Texture's colour into the factor input): narrowing,
        # REJECTED — the typed gate arity alone could never make.
        assert_eq(
            tf.invoke("/external/add_edge", f"0,0,{lerp},2"),
            False,
            "Vector -> Float (narrowing) is rejected",
        )
        assert_eq(ecount(tf), base + 2, "the rejected wire changed nothing")
        # Vector -> Vector (Texture into Lerp's other colour input): accepted.
        assert_eq(
            tf.invoke("/external/add_edge", f"0,0,{lerp},1"),
            True,
            "Vector -> Vector (exact) is accepted",
        )
        assert_eq(ecount(tf), base + 3, "the exact Vector wire landed")
        # Out-of-range port is still rejected (the arity gate is intact).
        assert_eq(
            tf.invoke("/external/add_edge", f"{scalar},3,{lerp},0"),
            False,
            "an out-of-range output port is rejected",
        )
        assert_eq(ecount(tf), base + 3, "the out-of-range wire changed nothing")

        # ── (C) typed-port lists are read-only ──────────────────────
        for path, what in (
            ("/external/node.2.input_types", "Multiply input_types"),
            (f"/external/node.{lerp}.output_types", "Lerp output_types"),
        ):
            rejected = False
            try:
                tf.intervene(path, "Float")
            except RpcError:
                rejected = True
            assert rejected, f"{what} is read-only"
        assert_eq(ncount(tf), 6, "the read-only rejections changed nothing")

        # ── (D) color-coded pins (scene-as-data fill) ───────────────
        # A Float output paints green; a Vector output paints gold — a
        # connection's validity is legible from the pin colours alone.
        assert_color(port_fill(tf, f"{G}#oport_{scalar}_0"), FLOAT_GREEN, "Scalar's Float output pin")
        assert_color(port_fill(tf, f"{G}#oport_0_0"), VECTOR_GOLD, "Texture's Vector output pin")
        # Lerp's typed inputs colour-code per port: in0/in1 Vector (gold),
        # in2 the Float factor (green).
        assert_color(port_fill(tf, f"{G}#iport_{lerp}_0"), VECTOR_GOLD, "Lerp input 0 (Vector) pin")
        assert_color(port_fill(tf, f"{G}#iport_{lerp}_2"), FLOAT_GREEN, "Lerp input 2 (Float factor) pin")
        # The two type colours are distinct (the whole point of coding).
        assert FLOAT_GREEN != VECTOR_GOLD, "Float and Vector pins are distinguishable"


if __name__ == "__main__":
    sys.exit(run_demo("R898 §5.38 §5.40 — node-graph typed ports (blueprint foundation)", body))
