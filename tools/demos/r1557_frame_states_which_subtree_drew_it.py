#!/usr/bin/env python3
"""R1557 §5.16 §5.18 §2 #2 — the frame states WHICH SUBTREE drew it.

R1556 gave this axis the frame's draw census: how many draw commands, paths,
path segments, clip layers, glyph runs and glyphs the encoded scene asks the
renderer for. It is one number per frame, so it says *how much* and never
*where* — and two frames that each cost four thousand glyphs somewhere
different read identically while wanting opposite fixes.

`scene/draw_profile` is the other half, and the shape of every profiler's
central artifact: a tree mirroring the painted scene, each node carrying an
INCLUSIVE cost (itself and its whole subtree) and an EXCLUSIVE one (itself
alone). A flame graph is that tree drawn sideways.

The attribution is taken as a DIFFERENCE of censuses of the encoded scene —
one before a subtree is walked, one after — so a subtree the §5.16 fragment
cache replayed lands in it exactly as a freshly-encoded one does, and nothing a
node draws can escape its own row.

This demo asserts:

  (A) The reply is on the wire, typed, complete, and `rpc/schema` DESCRIBES it:
      the published key set and the live response's are compared against each
      other (R1539's discipline — a response shape nothing checks is a comment).

  (B) **The profile measures the frame that actually ran.** The root's inclusive
      total equals `scene/frame_timings`' `last.draw`, field for field. The
      profile is produced by re-encoding the retained paint scene into a COLD
      fragment cache while the live frame was served by a warm one, so this
      equality is also the first out-of-crate check of the fragment cache's own
      correctness invariant: a replayed subtree draws what it drew when encoded.

  (C) **The attribution is a PARTITION.** Summing `own` over every node in the
      tree gives the root's `total`, exactly, in all six units. Overlapping
      estimates cannot do this; neither can a tree with a node whose work landed
      outside the span being measured, nor a saturating subtraction that clamped.

  (D) **Text is attributed to the leaf that drew it.** Every glyph in the frame
      belongs to a `Text` node's `own`, and no `Container` claims one. This is
      the term no node census can reach — a `Text` leaf is one node whether it
      holds two glyphs or four thousand — and no Qt surface reports it per item.

  (E) **THE ROUND'S OWN CASE — where, not just how much.** Widening every row's
      label 24 -> 1,536 characters leaves the frame's node counts identical
      (R1556's case) AND leaves the profile's SHAPE identical — same node count,
      same paths, same kinds — while moving the glyphs into exactly the rows
      whose labels grew. The header's own cost does not move by one glyph. That
      is the question `last.draw` cannot answer and this one is for.

  (F) **Scale invariance, per subtree.** Growing the model 100 -> 1,000,000 rows
      leaves the whole profile — every path, every count — byte-identical. R1538
      claimed per-frame work is bounded by what is visible; this states it for
      every subtree at once rather than for the frame as a sum.

  (G) **The guard can fail.** On the eager arm the profile grows with the model:
      more rows, more paths in the profile, more nodes. A guard that only ever
      measures the passing case cannot fail (R1527).

  (H) **A row's path is an address.** The profile's own path for the External's
      container, extended by the introspect tail, resolves through
      `scene/query` — two independent derivations (the paint walk and
      `Scene::lookup_path_ref`) agreeing on one string.

  (I) **Ranking is by a NAMED unit, and truncation is never silent.** `glyphs`
      and `paths` rank differently, an unknown unit is refused with the valid
      set echoed, and a `depth` limit reports what it cut three ways
      (`children_omitted`, `nodes`, `nodes_total`).

ZERO-FLAKE: not one assertion names a microsecond, a frame rate, or a machine.
Every claim is a count, an ordering, an equality or a presence. Frames are
driven by the window's own `frame_count`, never by a sleep.

Run from the workspace root:
    cargo build -p hello-scene-scale --release
    python3 tools/demos/r1557_frame_states_which_subtree_drew_it.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

APP = "hello-scene-scale"
LIST_TAG = "scale"
EXT = f"/{LIST_TAG}/external"

DRAW_FIELDS = ("draws", "paths", "path_segments", "layers", "glyph_runs", "glyphs")
ROW_KEYS = (
    "path",
    "segment",
    "kind",
    "tag",
    "total",
    "own",
    "children",
    "children_omitted",
)
OUTCOME_KEYS = (
    "root",
    "path",  # R1558 — the scope this reply was rooted at, or null
    "nodes",
    "nodes_total",
    "depth",
    "heaviest_by",
    "heaviest",
)
RANK_KEYS = ("path", "kind", "tag", "own")


# ---------------------------------------------------------------- driving ----


def drive_frame(tf: RpcSubprocess, desc: str) -> dict:
    """Drive real paints until `frame_count` advances, then read the sample.

    `scene/screenshot` forces a real view + layout + encode + submit through the
    live pipeline, which is the only thing that records a frame and the only
    thing that leaves a retained paint scene for the profile to attribute.
    """
    baseline = int(tf.frame_timings()["frame_count"])

    def advanced() -> bool:
        try:
            if int(tf.frame_timings()["frame_count"]) > baseline:
                return True
        except RpcError:
            pass
        tf.request("scene/screenshot", {"path": ""})
        return False

    wait_until(advanced, desc=desc)
    return tf.frame_timings()


def profile(tf: RpcSubprocess, **params) -> dict:
    return tf.request("scene/draw_profile", params or {}).result


def set_rows(tf: RpcSubprocess, rows: int) -> None:
    tf.intervene(f"{EXT}/rows", rows)
    assert_eq(tf.query(f"{EXT}/rows"), rows, f"the model took rows={rows}")


def set_label_chars(tf: RpcSubprocess, chars: int) -> None:
    tf.intervene(f"{EXT}/label_chars", chars)
    assert_eq(tf.query(f"{EXT}/label_chars"), chars, f"the model took chars={chars}")


# ---------------------------------------------------------------- walking ----


def walk(node: dict):
    yield node
    for child in node["children"]:
        yield from walk(child)


def sum_own(root: dict, field: str) -> int:
    return sum(n["own"][field] for n in walk(root))


def shape(root: dict) -> list[tuple[str, str]]:
    """The profile's structure with every count stripped off."""
    return [(n["path"], n["kind"]) for n in walk(root)]


def rows_of(root: dict) -> dict[str, dict]:
    """Every list row's node, keyed by its `scale#<n>` tag."""
    return {
        n["tag"]: n
        for n in walk(root)
        if (n["tag"] or "").startswith(f"{LIST_TAG}#")
        and n["tag"][len(LIST_TAG) + 1 :].isdigit()
    }


def own_by_path(root: dict) -> dict[str, dict]:
    """Every node's EXCLUSIVE census, keyed by its address."""
    return {n["path"]: n["own"] for n in walk(root)}


def header_text(root: dict) -> dict[str, int]:
    """Each header button's own glyph count, keyed by the button's tag.

    Classified by ANCESTRY rather than by tag: a row's label is a `Text` leaf
    with no tag of its own, so "everything tagless" would sweep the rows in with
    the header and make the invariance below vacuous.
    """
    out: dict[str, int] = {}
    for node in walk(root):
        tag = node["tag"] or ""
        if not tag.startswith(f"{LIST_TAG}#") or tag[len(LIST_TAG) + 1 :].isdigit():
            continue
        out[tag] = sum(n["own"]["glyphs"] for n in walk(node))
    return out


# -------------------------------------------------------------- assertions ---


def assert_wire_shape(prof: dict, label: str) -> None:
    """(A) Present, typed, complete and mutually coherent — before any belief."""
    assert_eq(sorted(prof), sorted(OUTCOME_KEYS), f"{label}: outcome key set")
    root = prof["root"]
    assert root is not None, (
        f"{label}: `root` is null. This window has painted, so a null root is "
        f"an absent measurement wearing the shape of an empty scene"
    )
    for node in walk(root):
        assert_eq(sorted(node), sorted(ROW_KEYS), f"{label}: row key set at {node['path']}")
        assert isinstance(node["path"], str) and node["path"].startswith("/window["), (
            f"{label}: row path {node['path']!r} is not an address"
        )
        assert isinstance(node["kind"], str) and node["kind"], f"{label}: row kind"
        for group in ("total", "own"):
            census = node[group]
            assert_eq(
                sorted(census),
                sorted(DRAW_FIELDS),
                f"{label}: `{group}` at {node['path']} answers the published units",
            )
            for field in DRAW_FIELDS:
                value = census[field]
                assert isinstance(value, int) and not isinstance(value, bool), (
                    f"{label}: `{group}.{field}` must be an integer count, got {value!r}"
                )
                assert value >= 0, f"{label}: `{group}.{field}` is negative: {value}"
                assert value <= node["total"][field], (
                    f"{label}: {node['path']} own.{field}={node['own'][field]} exceeds "
                    f"total.{field}={node['total'][field]} — a node cannot draw more "
                    f"by itself than it and its subtree drew together"
                )
        for field in DRAW_FIELDS:
            children = sum(c["total"][field] for c in node["children"])
            assert_eq(
                node["own"][field] + children,
                node["total"][field],
                f"{label}: {node['path']} total.{field} is own + children's totals",
            )
    assert_eq(
        prof["nodes"],
        len(list(walk(root))),
        f"{label}: `nodes` counts the rows actually emitted",
    )


def assert_schema_describes_it(tf: RpcSubprocess, prof: dict) -> None:
    """(A) `rpc/schema` published this shape, and the live reply matches it."""
    schema = tf.request("rpc/schema", {}).result
    types = {t["name"]: t for t in schema["types"]}
    for name in ("DrawProfileOutcome", "DrawProfileRow", "DrawProfileRank", "DrawProfileWork"):
        assert name in types, (
            f"`rpc/schema` does not publish {name}. A response shape no agent "
            f"can discover is a shape only the source knows"
        )
    published = [f["name"] for f in types["DrawProfileOutcome"]["shape"]["fields"]]
    assert_eq(
        sorted(published),
        sorted(prof),
        "the published `DrawProfileOutcome` key set is the one the wire answers",
    )
    row_fields = [f["name"] for f in types["DrawProfileRow"]["shape"]["fields"]]
    assert_eq(
        sorted(row_fields),
        sorted(prof["root"]),
        "the published `DrawProfileRow` key set is the one a row answers",
    )
    work_fields = [f["name"] for f in types["DrawProfileWork"]["shape"]["fields"]]
    assert_eq(
        sorted(work_fields),
        sorted(DRAW_FIELDS),
        "the published `DrawProfileWork` key set is the six draw units",
    )
    # The recursion is declared, not merely implied by the JSON being nested.
    children = next(
        f for f in types["DrawProfileRow"]["shape"]["fields"] if f["name"] == "children"
    )
    assert_eq(children.get("of"), "DrawProfileRow", "`children` names its own type")


def assert_measures_the_frame_that_ran(prof: dict, timings: dict, label: str) -> None:
    """(B) The root's inclusive total IS `scene/frame_timings`' `last.draw`."""
    drawn = timings["last"]["draw"]
    total = prof["root"]["total"]
    for field in DRAW_FIELDS:
        assert_eq(
            total[field],
            drawn[field],
            f"{label}: root total.{field} is the frame's own `last.draw.{field}` — "
            f"the profile re-encodes into a COLD fragment cache while the live "
            f"frame was served by a warm one, so a mismatch is either a "
            f"mis-attribution here or a cache that does not replay what it stored",
        )
    for field in ("draws", "paths", "path_segments", "glyph_runs", "glyphs"):
        assert total[field] > 0, (
            f"{label}: the frame drew no {field}. Zero here would satisfy every "
            f"invariance assertion below on a column of zeros"
        )
    assert total["layers"] >= 1, (
        f"{label}: the list is inside a scroll viewport, which pushes a clip "
        f"layer; `layers=0` means the clip never reached the encoding"
    )


def assert_partition(prof: dict, label: str) -> None:
    """(C) Every node's exclusive cost sums to the root's inclusive one."""
    root = prof["root"]
    for field in DRAW_FIELDS:
        assert_eq(
            sum_own(root, field),
            root["total"][field],
            f"{label}: every node's own.{field} sums to the root's total.{field} — "
            f"the attribution is a partition of the frame, not a set of estimates",
        )


def assert_text_is_attributed(prof: dict, label: str) -> None:
    """(D) The glyphs belong to text leaves, and to nothing else."""
    root = prof["root"]
    by_text = sum(n["own"]["glyphs"] for n in walk(root) if n["kind"] == "Text")
    assert_eq(
        by_text,
        root["total"]["glyphs"],
        f"{label}: every glyph in the frame belongs to a Text node's own cost",
    )
    for node in walk(root):
        if node["kind"] == "Container":
            assert_eq(
                node["own"]["glyphs"],
                0,
                f"{label}: container {node['path']} claims glyphs it did not draw",
            )
        if node["kind"] == "Text":
            assert_eq(
                node["own"]["paths"],
                0,
                f"{label}: text {node['path']} claims paths — glyph outlines are "
                f"resolved downstream of the encoding, so text and geometry are "
                f"disjoint units, not overlapping ones",
            )


# --------------------------------------------------------------------- run ---


def main() -> None:
    with RpcSubprocess(APP) as tf:
        timings = drive_frame(tf, "the first painted frame")
        prof = profile(tf)

        # (A)
        assert_wire_shape(prof, "baseline")
        assert_schema_describes_it(tf, prof)
        assert_eq(prof["depth"], None, "no depth limit was asked for")
        assert_eq(prof["heaviest_by"], None, "no ranking was asked for")
        assert_eq(prof["heaviest"], [], "and none was produced")
        assert_eq(
            prof["nodes"], prof["nodes_total"], "an unpruned reply holds the whole profile"
        )
        assert prof["nodes"] > 10, (
            f"premise: this binding paints a real tree, got {prof['nodes']} nodes"
        )
        assert_eq(prof["root"]["segment"], None, "the root consumes no path segment")

        # (B) and (C)
        assert_measures_the_frame_that_ran(prof, timings, "baseline")
        assert_partition(prof, "baseline")

        # (D)
        assert_text_is_attributed(prof, "baseline")

        # (H) a row's path is an address two independent derivations agree on.
        ext_row = next(
            n for n in walk(prof["root"]) if n["tag"] == LIST_TAG
        )
        resolved = tf.request(
            "scene/query", {"path": f"{ext_row['path']}/external/rows"}
        ).result
        assert_eq(
            resolved,
            tf.query(f"{EXT}/rows"),
            "the profile's own path for the External's container resolves through "
            "`scene/query` to the same model value the short form reaches — the "
            "paint walk and `Scene::lookup_path_ref` agree on one address",
        )

        # (H, continued) A `Scroll` consumes no path segment, so its content is
        # reported at the scroll's own address — the rule `Scene::hit_test`,
        # `Scene::lookup_path_ref` and `scene/locate` all follow. Asserted on
        # the wire because a profile that invented a segment here would publish
        # addresses no other surface resolves, and every same-shape comparison
        # below would still pass: a systematically wrong path is still
        # systematically equal to itself.
        scrolls = [n for n in walk(prof["root"]) if n["kind"] == "Scroll"]
        assert scrolls, "premise: this binding paints its list inside a scroll"
        for scroll in scrolls:
            assert_eq(
                len(scroll["children"]),
                1,
                f"a Scroll has exactly one content child ({scroll['path']})",
            )
            content = scroll["children"][0]
            assert_eq(
                content["segment"],
                None,
                f"the content of {scroll['path']} consumes no path segment",
            )
            assert_eq(
                content["path"],
                scroll["path"],
                "…so it is reported at the scroll's own address, which is where "
                "`Scene::lookup_path_ref` reaches it",
            )
            assert_eq(
                scroll["own"]["layers"],
                1,
                f"{scroll['path']} owns the clip layer it pushes, and the "
                f"content does not",
            )
            assert_eq(content["own"]["layers"], 0, "…and the content does not")

        # (E) THE ROUND'S CASE — the cost moves, the shape does not.
        base_rows = rows_of(prof["root"])
        assert len(base_rows) >= 5, (
            f"premise: the windowed list paints rows, got {len(base_rows)}"
        )
        narrow_shape = shape(prof["root"])
        narrow_nodes = {f: prof["root"]["total"][f] for f in DRAW_FIELDS}
        header_own = header_text(prof["root"])
        assert_eq(
            sorted(header_own),
            ["scale#grow", "scale#mode", "scale#width"],
            "premise: the three header buttons carry the only text outside the list",
        )

        set_label_chars(tf, 1_536)
        wide_timings = drive_frame(tf, "a frame with 1,536-character labels")
        wide = profile(tf)
        assert_wire_shape(wide, "wide labels")
        assert_measures_the_frame_that_ran(wide, wide_timings, "wide labels")
        assert_partition(wide, "wide labels")
        assert_text_is_attributed(wide, "wide labels")

        assert_eq(
            shape(wide["root"]),
            narrow_shape,
            "widening every label leaves the profile's SHAPE identical — same "
            "nodes, same paths, same kinds. Only the cost inside them moved",
        )
        assert wide["root"]["total"]["glyphs"] > narrow_nodes["glyphs"] * 10, (
            f"premise: 64x the characters must move the frame's glyph count "
            f"({narrow_nodes['glyphs']} -> {wide['root']['total']['glyphs']})"
        )
        wide_rows = rows_of(wide["root"])
        assert_eq(
            sorted(wide_rows), sorted(base_rows), "the same rows are on screen"
        )
        for tag, node in wide_rows.items():
            assert node["total"]["glyphs"] > base_rows[tag]["total"]["glyphs"] * 10, (
                f"row {tag}: the label grew 64x and its glyph attribution did not "
                f"({base_rows[tag]['total']['glyphs']} -> {node['total']['glyphs']})"
            )
        # The header is untouched by the row ladder — to the glyph — EXCEPT for
        # the one button that displays the width itself, which gains exactly the
        # digits `24` -> `1536` added. Nothing here is "roughly unchanged": the
        # profile accounts for every glyph the frame gained.
        wide_header = header_text(wide["root"])
        assert_eq(
            {k: v for k, v in wide_header.items() if k != "scale#width"},
            {k: v for k, v in header_own.items() if k != "scale#width"},
            "the header buttons that do not display the width are untouched by "
            "the row ladder, to the glyph",
        )
        assert_eq(
            wide_header["scale#width"] - header_own["scale#width"],
            len("1536") - len("24"),
            "…and the one that DOES display it gains exactly the digits it "
            "gained — the frame's glyph total moved by tens of thousands, and "
            "the profile accounts for every one of them by node",
        )

        set_label_chars(tf, 24)
        drive_frame(tf, "back to narrow labels")

        # (F) scale invariance, per subtree.
        set_rows(tf, 100)
        drive_frame(tf, "the model at 100 rows")
        small = profile(tf)
        set_rows(tf, 1_000_000)
        big_timings = drive_frame(tf, "the model at 1,000,000 rows")
        big = profile(tf)
        assert_wire_shape(big, "1e6 rows")
        assert_measures_the_frame_that_ran(big, big_timings, "1e6 rows")
        assert_partition(big, "1e6 rows")
        assert_eq(
            shape(big["root"]),
            shape(small["root"]),
            "growing the model four orders of magnitude leaves the profile's "
            "shape identical — every path, every kind. R1538 claimed per-frame "
            "work is bounded by what is visible; this states it for every "
            "subtree at once instead of for the frame as a sum",
        )
        small_own = own_by_path(small["root"])
        big_own = own_by_path(big["root"])
        moved = sorted(p for p in small_own if small_own[p] != big_own[p])
        assert_eq(
            moved,
            [f"/window[main]/0/{LIST_TAG}#grow/0"],
            "…and the ONLY node whose exclusive cost moved across four orders of "
            "magnitude is the header label that PRINTS the row count, which "
            "gained the digits it prints. Every other node in the tree drew "
            "exactly what it drew at a ten-thousandth of the model",
        )
        assert_eq(
            big_own[moved[0]]["glyphs"] - small_own[moved[0]]["glyphs"],
            len("1000000") - len("100"),
            "…by exactly that many glyphs",
        )

        # (G) the guard can fail: the eager arm builds one node per row.
        set_rows(tf, 100)
        drive_frame(tf, "back to 100 rows")
        tf.intervene(f"{EXT}/eager", True)
        assert_eq(tf.query(f"{EXT}/eager"), True, "the model took the eager arm")
        drive_frame(tf, "the eager arm at 100 rows")
        eager_small = profile(tf)
        assert_partition(eager_small, "eager 100")
        set_rows(tf, 1_000)
        drive_frame(tf, "the eager arm at 1,000 rows")
        eager_big = profile(tf)
        assert_partition(eager_big, "eager 1000")
        assert eager_big["nodes"] > eager_small["nodes"] * 5, (
            f"the eager arm builds one node per row, so the PROFILE must grow "
            f"with the model ({eager_small['nodes']} -> {eager_big['nodes']}). "
            f"A guard that only ever measures the passing case cannot fail"
        )
        assert eager_big["root"]["total"]["paths"] > eager_small["root"]["total"]["paths"], (
            "…and so must the work it attributes"
        )
        tf.intervene(f"{EXT}/eager", False)
        set_rows(tf, 1_000)
        drive_frame(tf, "back to the virtual arm")

        # (I) ranking by a named unit; loud truncation; refused units.
        ranked = profile(tf, heaviest_by="glyphs", limit=3)
        assert_eq(ranked["heaviest_by"], "glyphs", "the unit is echoed")
        assert_eq(len(ranked["heaviest"]), 3, "the limit is honoured")
        for entry in ranked["heaviest"]:
            assert_eq(sorted(entry), sorted(RANK_KEYS), "rank row key set")
        glyph_counts = [e["own"]["glyphs"] for e in ranked["heaviest"]]
        assert_eq(
            glyph_counts, sorted(glyph_counts, reverse=True), "ranked most-first"
        )
        assert_eq(
            ranked["heaviest"],
            profile(tf, heaviest_by="glyphs", limit=3)["heaviest"],
            "the same scene ranks identically twice — the ordering is total, "
            "because ties break on the path rather than on walk luck",
        )
        by_paths = profile(tf, heaviest_by="paths", limit=3)
        assert_eq(by_paths["heaviest_by"], "paths", "a different unit is echoed")
        assert by_paths["heaviest"] != ranked["heaviest"], (
            "ranking by geometry and ranking by text name different nodes — "
            "which is exactly why the unit is the caller's to name and pinion "
            "ships no single 'heaviest' scalar"
        )
        assert all(e["own"]["paths"] > 0 for e in by_paths["heaviest"]), (
            "a `paths` ranking names nodes that drew paths"
        )

        pruned = profile(tf, depth=1)
        assert_eq(pruned["depth"], 1, "the limit is echoed")
        assert pruned["nodes"] < pruned["nodes_total"], (
            f"depth=1 must cut this tree ({pruned['nodes']} of "
            f"{pruned['nodes_total']})"
        )
        assert_eq(
            pruned["nodes_total"],
            profile(tf)["nodes"],
            "`nodes_total` is the whole profile, whatever `depth` emitted",
        )
        cut = sum(n["children_omitted"] for n in walk(pruned["root"]))
        assert cut > 0, "…and every parent that lost children says how many"
        assert_eq(
            pruned["root"]["total"],
            profile(tf)["root"]["total"],
            "pruning the REPLY does not change what was measured",
        )

        for bad, tag in (
            ({"heaviest_by": "milliseconds"}, "UnknownUnit"),
            ({"depth": "deep"}, "MalformedParam"),
            ({"limit": -1}, "MalformedParam"),
        ):
            try:
                profile(tf, **bad)
            except RpcError as exc:
                assert tag in str(exc), f"{bad} must be refused as {tag}, got {exc}"
                if tag == "UnknownUnit":
                    for unit in DRAW_FIELDS:
                        assert unit in str(exc), (
                            f"the refusal teaches the valid set; {unit} is missing "
                            f"from {exc}"
                        )
            else:
                raise AssertionError(f"{bad} was accepted")

        # `rpc/methods` knows the method, and classes it as a read.
        methods = {m["name"]: m["occ"] for m in tf.request("rpc/methods", {}).result["methods"]}
        assert_eq(
            methods.get("scene/draw_profile"),
            "read",
            "the method is discoverable and classed — profiling a frame changes "
            "no scene state",
        )


if __name__ == "__main__":
    run_demo("R1557 the frame states which subtree drew it", main)
