#!/usr/bin/env python3
"""R1514 §5.49 §2 #7 — the wire carries every facet a style can declare.

`BoxStyle` is `#[non_exhaustive]`, so no crate downstream of `pinion-core` can
destructure it. `box_style_to_json` — the §2 #7 projection an AI client reads a
box's look through, without OCR — was therefore a HAND LIST of five keys, and a
sixth facet added to the type would simply never have reached the wire. Nothing
could have noticed: every fixture in that crate's suite was written from the
same hand list, so all of them agreed with each other about a set none of them
could see.

R1514 makes the key set `pinion_core::style::BoxFacet::ALL` and matches it
exhaustively, which turns a missing facet into a compile error. That is the
in-crate half. This is the other half, over a real socket to a real binding:
the object a client actually receives carries the whole census, every key
carries a readable value where it is declared, and a readable EMPTY form where
it is not — so "this box has no border" and "this build forgot borders" are
distinguishable by the client, which is the entire point of scene-as-data.

Two bindings are used because no single one declares all five facets:
`hello-gradient` declares fill / corner_radius / gradient, `hello-card`
declares fill / border / corner_radius / shadows. Their union is the census,
and the claim under test is about the projection, not about either binding.

ZERO-FLAKE: every assertion reads a structure this demo asked for by name.
Nothing waits on wall-clock, pixels, or a frame count.

Run from the workspace root:
    cargo build -p hello-gradient -p hello-card --release
    python3 tools/demos/r1514_wire_carries_every_facet.py
"""

from __future__ import annotations

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    walk_nodes,
)

# The census, restated from the client's side.
#
# R1506's lesson was to delete constant mirrors from demos and read invariants
# out of the tree instead. This is deliberately NOT that: the wire key set is
# the contract a client codes against, and a check that only asked the tree to
# agree with itself would pass just as happily if every box on the wire lost
# the same key. An independent restatement is the only shape that can fail when
# the projection and the type drift apart together.
CENSUS = ["border", "corner_radius", "fill", "gradient", "shadows"]

# Which binding declares which facet. Chosen because the union is the census —
# see the module docstring.
BINDINGS = {
    "hello-gradient": ["fill", "corner_radius", "gradient"],
    "hello-card": ["fill", "corner_radius", "border", "shadows"],
}

BOX_NODES = ("Box", "Container")


def is_declared(facet: str, style: dict) -> bool:
    """Whether this style declares `facet` — i.e. carries something other than
    the documented empty form. Mirrors `BoxFacet::is_declared`'s meaning
    (differs from the default) at the wire's vocabulary."""
    value = style.get(facet)
    if facet in ("border", "gradient"):
        return value is not None
    if facet == "shadows":
        return bool(value)
    if facet == "corner_radius":
        return value != 0
    # `fill` defaults to fully transparent; anything opaque is a declaration.
    return isinstance(value, dict) and value.get("a", 0) != 0


def body() -> None:
    declared_somewhere: dict[str, tuple[str, str]] = {}
    box_node_total = 0
    text_node_total = 0

    for example, expected_facets in BINDINGS.items():
        with RpcSubprocess(example, boot_grace=1.5) as tf:
            nodes = [
                (path, node.get("type"), node.get("style"))
                for path, node in walk_nodes(tf.snapshot(source="paint"))
            ]

            boxes = [
                (path, style)
                for path, kind, style in nodes
                if kind in BOX_NODES and isinstance(style, dict)
            ]
            texts = [
                (path, style)
                for path, kind, style in nodes
                if kind == "Text" and isinstance(style, dict)
            ]

            # ── (A) premise: there is something to read ─────────────────────
            # An empty scene would satisfy every "for each box" assertion
            # below vacuously, which is the shape of a guard that cannot fail.
            assert boxes, f"★A[{example}]: the paint scene has box-styled nodes"
            assert texts, f"A[{example}]: and text nodes, used as the control"
            box_node_total += len(boxes)
            text_node_total += len(texts)

            # ── (B) every box carries the whole census ──────────────────────
            for path, style in boxes:
                assert_eq(
                    sorted(style.keys()),
                    CENSUS,
                    f"★B[{example}]{path}: the style object IS BoxFacet::ALL",
                )

            # ── (C) the control ─────────────────────────────────────────────
            # (B) would also hold if the projection stamped these five keys
            # onto every style object in the tree, census or not. A `TextNode`
            # carries a `TextStyle` and must NOT look like a box — that is what
            # makes (B) a statement about box facets rather than about JSON.
            for path, style in texts:
                assert sorted(style.keys()) != CENSUS, (
                    f"★C[{example}]{path}: a text style is not a box style; if "
                    f"it were, (B) would be measuring nothing"
                )
                assert "font_size_px" in style, (
                    f"C[{example}]{path}: and it carries its own vocabulary"
                )

            # ── (D) the facets this binding declares are readable ───────────
            for facet in expected_facets:
                witnesses = [(p, s) for p, s in boxes if is_declared(facet, s)]
                assert witnesses, (
                    f"★D[{example}]: `{facet}` is declared by some box here — "
                    f"a key that is never anything but its empty form proves "
                    f"nothing about whether the projection can carry it"
                )
                declared_somewhere[facet] = (example, witnesses[0][0])

            # ── (E) an undeclared facet reports its EMPTY form, not junk ────
            # A client has to tell "this box has no border" from "this build
            # lost borders", and only a documented empty value does that.
            for path, style in boxes:
                if not is_declared("border", style):
                    assert_eq(style["border"], None, f"E[{example}]{path}: border absent = null")
                if not is_declared("gradient", style):
                    assert_eq(style["gradient"], None, f"E[{example}]{path}: gradient absent = null")
                if not is_declared("shadows", style):
                    assert_eq(style["shadows"], [], f"E[{example}]{path}: shadows absent = []")

            # ── (F) declared values are STRUCTURED, not merely present ──────
            # `box_style_to_json` could satisfy (B) by emitting the five keys
            # with placeholder values. §2 #7 is "queryable as data": each
            # declared facet has to arrive as something a client can read.
            for path, style in boxes:
                fill = style["fill"]
                assert isinstance(fill, dict) and set("rgba") <= set(fill), (
                    f"F[{example}]{path}: fill is an RGBA object, got {fill!r}"
                )
                assert isinstance(style["corner_radius"], int), (
                    f"F[{example}]{path}: corner_radius is a number"
                )

                border = style["border"]
                if border is not None:
                    assert isinstance(border.get("color"), dict), (
                        f"★F[{example}]{path}: a border reports its colour"
                    )
                    assert isinstance(border.get("width"), int) and border["width"] > 0, (
                        f"★F[{example}]{path}: …and a positive width: {border!r}"
                    )
                    assert border.get("placement") in ("Inside", "Center", "Outside"), (
                        f"★F[{example}]{path}: …and which side of the edge it sits on"
                    )

                gradient = style["gradient"]
                if gradient is not None:
                    stops = gradient.get("stops")
                    assert isinstance(stops, list) and len(stops) >= 2, (
                        f"★F[{example}]{path}: a gradient reports its ramp: {gradient!r}"
                    )
                    for stop in stops:
                        assert isinstance(stop.get("color"), dict), (
                            f"★F[{example}]{path}: every stop carries a colour"
                        )
                        assert isinstance(stop.get("offset"), float), (
                            f"★F[{example}]{path}: …at a position along the ramp"
                        )
                    assert isinstance(gradient.get("geometry"), dict), (
                        f"★F[{example}]{path}: …and the ramp's geometry"
                    )
                    assert gradient.get("extend") in ("Pad", "Repeat", "Reflect"), (
                        f"★F[{example}]{path}: …and what happens past its ends"
                    )

                for shadow in style["shadows"]:
                    assert isinstance(shadow.get("color"), dict), (
                        f"★F[{example}]{path}: a shadow reports its colour"
                    )
                    assert isinstance(shadow.get("blur"), float), (
                        f"★F[{example}]{path}: …its blur, which is the whole of "
                        f"what a penumbra is"
                    )
                    assert isinstance(shadow.get("offset"), dict), (
                        f"★F[{example}]{path}: …and where it falls"
                    )

    # ── (G) the union covers the census ─────────────────────────────────────
    # Per-binding, (D) only proves the facets that binding uses. Together they
    # have to account for every key in (B): a facet that no binding anywhere
    # can be seen declaring is a key whose presence on the wire is untested,
    # which is exactly the state `gradient` was in before R1514 — carried, but
    # by a hand list nobody had compared to the type.
    for facet in CENSUS:
        assert facet in declared_somewhere, (
            f"★G: some binding declares `{facet}`, so its key is witnessed "
            f"carrying a value and not only existing"
        )
    assert_eq(sorted(declared_somewhere), CENSUS, "★G: witnessed set IS the census")
    assert box_node_total >= 9, f"G: {box_node_total} box-styled nodes examined"
    assert text_node_total >= 9, f"G: {text_node_total} text nodes as control"


if __name__ == "__main__":
    sys.exit(run_demo("R1514 the wire carries every facet", body))
