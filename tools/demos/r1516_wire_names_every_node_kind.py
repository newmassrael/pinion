#!/usr/bin/env python3
"""R1516 §5.2 §2 #7 — every node kind reaches the wire under its own name.

`Scene` is `#[non_exhaustive]` — deliberately, so the game-engine variants its
own header names (`Mesh` / `Camera` / `Light`) can land without a major bump.
The cost is that no crate downstream can enumerate its variants or match them
without a wildcard, and the §2 #7 path an AI client reads a scene through ends
in two of them: `snapshot`'s `_ => SnapshotNode::Unknown` and
`snapshot_node_to_json`'s `_ => "Unknown"`. Both are right for the
version-skewed client their docs describe. Inside one workspace, where
`pinion-core` and `pinion-rpc` ship together, they mean something else: a
variant added to `Scene` would be painted on screen and arrive on the wire as
`"Unknown"`, with nothing failing.

`pinion_core::scene::SceneNodeKind` is the census that closes it, and the
in-crate half is a pair of compile errors plus
`r1516_every_census_kind_reaches_the_wire_under_its_own_name`, which runs the
whole census through the projection. This is the other half, over a real
socket to real bindings: what a client actually receives names its kind, and
the name is one the census knows.

The second claim here is the census's other fact. `SceneNodeKind::
carries_box_style` says which kinds carry a `BoxStyle`; on the wire that is
observable, because a `Box` and a `Container` carry the five `BoxFacet` keys
and nothing else does. `Path` is the control that makes the check mean
something: its `PathStyle` carries `fill` and `gradient` — two census names —
so a predicate that asked for "a style with a fill" would call it box-styled,
and this one does not.

Four bindings, chosen because their union covers nine of the ten kinds:
`hello-image` (Box / Container / Text / Image, plus an `External` in the state
mirror), `hello-node-editor` (Scroll / Path), `hello-textgrid` (TextGrid),
`hello-immediate-mode-canvas` (ImmediateModeNode). `Effect` is the tenth: it
is the §3 opaque escape with no geometry, no example paints one into a
snapshot tree, and it is covered by the unit test above rather than claimed
here on evidence that does not exist.

ZERO-FLAKE: every assertion reads a structure this demo asked for by name.
Nothing waits on wall-clock, pixels, or a frame count.

Run from the workspace root:
    cargo build -p hello-image -p hello-node-editor -p hello-textgrid \
        -p hello-immediate-mode-canvas --release
    python3 tools/demos/r1516_wire_names_every_node_kind.py
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
# R1506's lesson was to stop mirroring constants into demos and read invariants
# out of the tree instead. This is deliberately NOT that: the type tag is the
# discriminator a client codes against, and a check that only asked the tree to
# agree with itself would pass just as happily if a kind vanished from both the
# census and the projection at once. An independent restatement is the only
# shape that can fail when the two drift together.
CENSUS = [
    "Box",
    "Container",
    "Effect",
    "External",
    "Image",
    "ImmediateModeNode",
    "Path",
    "Scroll",
    "Text",
    "TextGrid",
]

# `SceneNodeKind::carries_box_style`, restated. The wire form of a `BoxStyle`
# is exactly the `BoxFacet` census (R1514), so this set is what "carries a
# BoxStyle" looks like to a client.
BOX_STYLED = {"Box", "Container"}
BOX_FACETS = ["border", "chrome", "corner_radius", "fill", "gradient", "shadows"]

# Which binding is read for which mirror. `hello-image`'s state scene is a
# single `External`: the same census governs both mirrors, and only the state
# mirror has an `External` to witness.
BINDINGS: dict[str, tuple[str, ...]] = {
    "hello-image": ("paint", "state"),
    "hello-node-editor": ("paint",),
    "hello-textgrid": ("paint",),
    "hello-immediate-mode-canvas": ("paint",),
}

# Every kind these four bindings paint between them. `Effect` is absent — see
# the module docstring; asserting it here would need a scene no example builds.
EXPECTED = sorted(set(CENSUS) - {"Effect"})


def is_box_styled(node: dict) -> bool:
    """Whether this node carries a `BoxStyle`, read the way a client must: the
    style object's key set IS the `BoxFacet` census. Not "has a style" (a
    `Text` has one) and not "has a fill" (a `Path` has one)."""
    style = node.get("style")
    return isinstance(style, dict) and sorted(style.keys()) == BOX_FACETS


def body() -> None:
    witnessed: dict[str, tuple[str, str, str]] = {}
    nodes_examined = 0
    scrolls_examined = 0

    for example, sources in BINDINGS.items():
        with RpcSubprocess(example, boot_grace=1.5) as tf:
            for source in sources:
                nodes = list(walk_nodes(tf.snapshot(source=source)))
                where = f"{example}/{source}"

                # ── (A) premise: there is something to read ─────────────────
                # An empty tree satisfies every "for each node" claim below
                # vacuously, which is the shape of a guard that cannot fail.
                assert nodes, f"★A[{where}]: the {source} scene has nodes"
                nodes_examined += len(nodes)

                for path, node in nodes:
                    kind = node.get("type")

                    # ── (B) the name is one the census knows ───────────────
                    assert kind in CENSUS, (
                        f"★B[{where}]{path}: node type {kind!r} is not a "
                        f"census kind — a variant reached the §2 #7 wire that "
                        f"`SceneNodeKind` does not name"
                    )
                    assert kind != "Unknown", (
                        f"★B[{where}]{path}: `Unknown` is the projection's "
                        f"word for a node it could not name; a client that "
                        f"reads it has been told nothing about what it sees"
                    )
                    witnessed.setdefault(kind, (example, source, path))

                    # ── (C) style presence follows the census ──────────────
                    # `carries_box_style` is a claim about kinds, and this is
                    # that claim where a client can check it.
                    assert_eq(
                        is_box_styled(node),
                        kind in BOX_STYLED,
                        f"★C[{where}]{path}: a {kind} carries a BoxStyle = "
                        f"{kind in BOX_STYLED}",
                    )

                    if kind in BOX_STYLED:
                        assert_eq(
                            sorted(node["style"].keys()),
                            BOX_FACETS,
                            f"C[{where}]{path}: …and it is the whole facet "
                            f"census, not a subset",
                        )

                    # ── (D) the clip is data, on the node the census says
                    #        clips ───────────────────────────────────────────
                    # `SceneNodeKind::clips_subtree` is true for `Scroll`
                    # alone. §2 #7 is "queryable as data": the window the
                    # renderers clip to has to be readable, or a client cannot
                    # tell hidden content from absent content.
                    if kind == "Scroll":
                        scrolls_examined += 1
                        viewport = node.get("viewport")
                        assert isinstance(viewport, dict), (
                            f"★D[{where}]{path}: a Scroll reports its clip "
                            f"viewport"
                        )
                        assert viewport["w"] > 0 and viewport["h"] > 0, (
                            f"★D[{where}]{path}: …with a real extent "
                            f"({viewport['w']}x{viewport['h']}); a degenerate "
                            f"window would make the clip unfalsifiable"
                        )
                        assert isinstance(node.get("content"), dict), (
                            f"★D[{where}]{path}: …and the subtree it clips, "
                            f"which is the content the viewport hides part of"
                        )
                        for axis_key in ("offset_x", "offset_y"):
                            assert isinstance(node.get(axis_key), int), (
                                f"D[{where}]{path}: …and {axis_key}, without "
                                f"which the visible slice is not derivable"
                            )

    # ── (E) the controls ────────────────────────────────────────────────────
    # (C) is a statement about box facets rather than about JSON only if some
    # node carries a style that is NOT a box style. `Path` is the sharp case:
    # its `PathStyle` carries `fill` and `gradient`, both census names, so a
    # looser predicate would have called it box-styled.
    path_kind = witnessed.get("Path")
    assert path_kind, "★E: a Path node was seen, so the control is not vacuous"
    text_kind = witnessed.get("Text")
    assert text_kind, "★E: and a Text node, whose TextStyle is the other control"

    # ── (F) the union covers every kind these bindings can show ─────────────
    # Per-binding, (B) only proves the kinds that binding paints. A kind no
    # binding anywhere is seen carrying is a kind whose wire name is untested —
    # which is the state every kind was in before this round.
    for kind in EXPECTED:
        assert kind in witnessed, (
            f"★F: some binding paints a {kind}, so its wire name is witnessed "
            f"on a real socket and not only in a unit fixture"
        )
    assert_eq(sorted(witnessed), EXPECTED, "★F: witnessed set IS the expected census slice")
    assert scrolls_examined >= 1, f"F: {scrolls_examined} Scroll nodes examined"
    assert nodes_examined >= 40, f"F: {nodes_examined} nodes examined"


if __name__ == "__main__":
    sys.exit(run_demo("R1516 the wire names every node kind", body))
