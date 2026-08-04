#!/usr/bin/env python3
"""R1558 §5.16 §5.18 §5.34 §2 #2 — the profile is rooted where you ask.

R1557 gave this axis a per-subtree attribution of the frame: a tree mirroring
the painted scene, each node carrying an INCLUSIVE cost and an EXCLUSIVE one.
It answers *where*, and it answers it for the whole window every time. Its only
narrowing knob, `depth`, trims the REPLY after a full window has been
re-encoded — so "profile this panel" was a question you asked by profiling
everything and reading one row.

`path` is the other axis. It names a subtree by the same `/window[main]/a/b`
address every profile row already publishes, and it scopes the MEASUREMENT:
only that subtree is re-encoded, only its nodes are attributed, and the cost of
asking falls with the subtree. That is what makes the profiler a bisection tool
— root, heaviest child, its heaviest child — where each step costs less than the
last instead of the same as the first.

The whole thing rests on one property, so this demo spends most of its
assertions on it: a subtree's draw work is INDEPENDENT OF THE CONTEXT IT IS
DRAWN IN. If that were false, a scoped profile would be a different
measurement wearing the same name and every drill-down would compare two
numbers that were never comparable.

This demo asserts:

  (A) The reply is typed and complete, and `rpc/schema` DESCRIBES the new key:
      the published census for `DrawProfileOutcome` and the live response's key
      set are compared against each other (R1539 — a response shape nothing
      checks is a comment).

  (B) **A scoped profile IS the subtree of the whole one.** Rooting at a row's
      own address reproduces that row exactly — every count in all six units,
      every descendant, every descendant's address — with one field deliberately
      different: a segment says where a node sits among its PARENT's children,
      and a scoped root has no parent.

  (C) **`path` scopes the measurement; `depth` scopes the reply.** Told apart
      from outside by `nodes_total`, which is the size of what was MEASURED: it
      shrinks under `path` and does not shrink under `depth`.

  (D) **The scopes PARTITION the frame.** Profiling each of the root's children
      separately and adding the totals to the root's own `own` reproduces the
      whole frame's census, field for field. Overlapping or truncated
      measurements cannot do this.

  (E) **Drilling down composes.** The heaviest row of a scoped profile is
      itself a scope, and rooting there reproduces that row — two levels deep,
      each answer strictly smaller than the last.

  (F) **A `Scroll` is addressed as itself.** Two rows share an address (a
      scroll's content consumes no segment), and the shared address resolves to
      the OUTER one — including the clip layer it owns. Stated on the wire
      because it is exactly where an addressing rule quietly disagreeing with a
      resolver would show up.

  (G) **Scoping survives scale.** The scoped profile of the list is identical
      across a 10,000x model, with an eager arm as the negative control where
      it must and does grow.

  (H) **Every refusal is named.** An address that reaches no node, a malformed
      prefix, an unknown window, a `path` that is not a string, and a request
      that names two different windows at once are five distinct typed
      failures — and "this address reached nothing" is not collapsed into
      "this window has no profile".

ZERO-FLAKE: not one assertion names a microsecond, a frame rate, or a machine.
Every claim is a count, an equality, an ordering or a presence. Frames are
driven by the window's own `frame_count`, never by a sleep.

Run from the workspace root:
    cargo build -p hello-scene-scale --release
    python3 tools/demos/r1558_profile_is_rooted_where_you_ask.py
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
OUTCOME_KEYS = (
    "root",
    "path",
    "nodes",
    "nodes_total",
    "depth",
    "heaviest_by",
    "heaviest",
)


# ---------------------------------------------------------------- driving ----


def drive_frame(tf: RpcSubprocess, desc: str) -> None:
    """Drive real paints until `frame_count` advances.

    `scene/screenshot` forces a real view + layout + encode + submit through the
    live pipeline, which is the only thing that leaves a retained paint scene
    for a profile to attribute.
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


def profile(tf: RpcSubprocess, **params) -> dict:
    return tf.request("scene/draw_profile", params or {}).result


def set_rows(tf: RpcSubprocess, rows: int) -> None:
    tf.intervene(f"{EXT}/rows", rows)
    assert_eq(tf.query(f"{EXT}/rows"), rows, f"the model took rows={rows}")


# ---------------------------------------------------------------- walking ----


def walk(node: dict):
    yield node
    for child in node["children"]:
        yield from walk(child)


def shape(root: dict) -> list[tuple[str, str, str | None]]:
    """The profile's structure with every count stripped off."""
    return [(n["path"], n["kind"], n["tag"]) for n in walk(root)]


def costs(root: dict) -> dict[str, tuple[dict, dict]]:
    """Every node's inclusive and exclusive census, keyed by its address.

    Addresses are not unique — a `Scroll`'s content is published at the
    scroll's own address — so the value is a LIST-free tuple only because the
    callers below compare trees that share the same duplication. `assert_subtree`
    walks structurally instead, and this is used only where the shape is already
    known equal.
    """
    return {n["path"]: (n["total"], n["own"]) for n in walk(root)}


def assert_subtree(scoped: dict, in_place: dict, at: str) -> None:
    """Assert a scoped profile's root reproduces `in_place`, node for node."""
    assert_eq(scoped["kind"], in_place["kind"], f"kind at {at}")
    assert_eq(scoped["tag"], in_place["tag"], f"tag at {at}")
    assert_eq(scoped["path"], in_place["path"], f"address at {at}")
    for field in DRAW_FIELDS:
        assert_eq(
            scoped["total"][field], in_place["total"][field], f"total.{field} at {at}"
        )
        assert_eq(scoped["own"][field], in_place["own"][field], f"own.{field} at {at}")
    assert_eq(
        len(scoped["children"]), len(in_place["children"]), f"child count at {at}"
    )
    for i, (a, b) in enumerate(zip(scoped["children"], in_place["children"])):
        assert_eq(a["segment"], b["segment"], f"segment at {at}/{i}")
        assert_subtree(a, b, f"{at}/{i}")


def node_count(node: dict) -> int:
    return sum(1 for _ in walk(node))


def assert_wire_shape(out: dict, label: str) -> None:
    assert_eq(sorted(out), sorted(OUTCOME_KEYS), f"outcome key set ({label})")


# ------------------------------------------------------------------- body ----


def main() -> None:
    with RpcSubprocess(APP) as tf:
        set_rows(tf, 1_000)
        drive_frame(tf, "the first real paint")

        # ---- (A) the reply is typed, and the census describes it ----------------
        whole = profile(tf)
        assert_wire_shape(whole, "whole window")
        assert_eq(whole["path"], None, "no scope asked for is echoed as null")
        root = whole["root"]
        assert root is not None, "premise: the window has painted"
        assert_eq(root["path"], "/window[main]/", "the whole window is rooted at the root")
        assert_eq(root["segment"], None, "a root consumes no path segment")
        assert_eq(node_count(root), whole["nodes"], "`nodes` counts the rows emitted")
        assert_eq(whole["nodes"], whole["nodes_total"], "nothing was pruned")

        published = {
            t["name"]: t for t in tf.request("rpc/schema", {}).result["types"]
        }
        census = published["DrawProfileOutcome"]["shape"]
        assert_eq(census["kind"], "object", "the outcome is published as an object")
        assert_eq(
            sorted(f["name"] for f in census["fields"]),
            sorted(whole),
            "the published census for `DrawProfileOutcome` and the live response "
            "agree on the key set — including this round's new `path`",
        )
        path_field = next(f for f in census["fields"] if f["name"] == "path")
        assert_eq(path_field["nullable"], True, "…and it is published as nullable")

        # ---- (B) a scoped profile IS the subtree of the whole one ---------------
        kids = root["children"]
        assert len(kids) >= 2, f"premise: the paint root has siblings ({len(kids)})"
        assert_eq(
            len({k["path"] for k in kids}),
            len(kids),
            "premise: the root's children have distinct addresses",
        )

        for kid in kids:
            scoped = profile(tf, path=kid["path"])
            assert_wire_shape(scoped, f"scoped to {kid['path']}")
            assert_eq(scoped["path"], kid["path"], "the scope is echoed verbatim")
            assert_eq(
                scoped["root"]["segment"],
                None,
                f"{kid['path']} is a root now, so it consumes no segment — the one "
                f"field a scoped profile MUST report differently, because a segment "
                f"says where a node sits among its parent's children",
            )
            assert kid["segment"] is not None, "…and in place it had one"
            assert_subtree(scoped["root"], kid, kid["path"])
            assert_eq(
                scoped["nodes_total"],
                node_count(kid),
                f"the profile of {kid['path']} holds exactly that subtree's nodes",
            )
            assert scoped["nodes_total"] < whole["nodes_total"], (
                f"…and strictly fewer than the window's ({scoped['nodes_total']} of "
                f"{whole['nodes_total']}) — the measurement was scoped, not relabelled"
            )

        # ---- (C) `path` scopes the measurement; `depth` scopes the reply -------
        pruned = profile(tf, depth=0)
        assert_eq(pruned["nodes"], 1, "depth=0 emits the root alone")
        assert_eq(
            pruned["nodes_total"],
            whole["nodes_total"],
            "…and `nodes_total` is untouched, because the whole window was still "
            "MEASURED — `depth` trims a reply, never a measurement",
        )
        assert_eq(pruned["path"], None, "a pruned whole-window reply names no scope")
        assert pruned["root"]["children_omitted"] > 0, "truncation is never silent"

        heaviest_kid = max(kids, key=lambda k: k["total"]["glyphs"])
        scoped = profile(tf, path=heaviest_kid["path"])
        assert scoped["nodes_total"] < pruned["nodes_total"], (
            f"`path` moves the number `depth` cannot ({scoped['nodes_total']} vs "
            f"{pruned['nodes_total']}) — that pair is how the two axes are told "
            f"apart from outside"
        )
        assert_eq(scoped["nodes"], scoped["nodes_total"], "and nothing was pruned")

        # A scoped reply prunes too, and the two axes compose without interfering.
        both = profile(tf, path=heaviest_kid["path"], depth=0)
        assert_eq(both["nodes"], 1, "depth=0 inside a scope emits its root alone")
        assert_eq(
            both["nodes_total"],
            scoped["nodes_total"],
            "…over the same measurement the scope took",
        )
        assert_eq(both["root"]["total"], scoped["root"]["total"], "…of the same work")

        # ---- (D) the scopes PARTITION the frame ---------------------------------
        parts = [profile(tf, path=k["path"])["root"] for k in kids]
        for field in DRAW_FIELDS:
            assert_eq(
                root["own"][field] + sum(p["total"][field] for p in parts),
                root["total"][field],
                f"the root's own {field} plus every child measured SEPARATELY "
                f"reproduces the frame — the scopes are disjoint and exhaustive",
            )
        assert root["total"]["glyphs"] > 0, "premise: the frame drew text"
        assert root["total"]["paths"] > 0, "premise: the frame drew geometry"

        # ---- (E) drilling down composes ----------------------------------------
        ranked = profile(tf, path=heaviest_kid["path"], heaviest_by="glyphs", limit=1)
        assert_eq(ranked["heaviest_by"], "glyphs", "the unit is echoed inside a scope")
        assert len(ranked["heaviest"]) == 1, "the limit is honoured inside a scope"
        deeper_path = ranked["heaviest"][0]["path"]
        assert deeper_path.startswith(heaviest_kid["path"]), (
            f"a scoped ranking names addresses INSIDE the scope: {deeper_path} is "
            f"not under {heaviest_kid['path']}"
        )
        in_place_deeper = next(
            n for n in walk(scoped["root"]) if n["path"] == deeper_path
        )
        deeper = profile(tf, path=deeper_path)
        assert_eq(deeper["root"]["path"], deeper_path, "the drill-down rooted there")
        assert_eq(
            deeper["root"]["total"],
            in_place_deeper["total"],
            "…and reproduces what the enclosing profile said that row cost. Two "
            "levels of narrowing, each answer the same as the one above it",
        )
        assert deeper["nodes_total"] <= scoped["nodes_total"], (
            "…over a measurement no larger than the one it came from"
        )

        # ---- (F) a `Scroll` is addressed as itself ------------------------------
        scrolls = [n for n in walk(root) if n["kind"] == "Scroll"]
        assert scrolls, "premise: this binding paints its list inside a scroll"
        scroll = scrolls[0]
        assert_eq(len(scroll["children"]), 1, "a Scroll has one content child")
        assert_eq(
            scroll["children"][0]["path"],
            scroll["path"],
            "premise: the content consumes no segment, so two rows share one address",
        )
        scoped_scroll = profile(tf, path=scroll["path"])
        assert_eq(
            scoped_scroll["root"]["kind"],
            "Scroll",
            "the shared address resolves to the OUTER node — the addressing rule "
            "and the resolver agree about which of the two it names",
        )
        assert_eq(
            scoped_scroll["root"]["own"]["layers"],
            scroll["own"]["layers"],
            "…and the scoped root still owns the clip layer it pushes, which a "
            "profile of its content alone would not have",
        )
        assert scroll["own"]["layers"] > 0, "premise: the scroll really does clip"
        assert_subtree(scoped_scroll["root"], scroll, scroll["path"])

        # …and an address that passes THROUGH the scroll resolves too. This is
        # where two derivations of one addressing rule would come apart: the
        # paint walk gives a scroll's content no segment, and the resolver is
        # separately Scroll-transparent. A row inside the list is addressed
        # across that seam by both, or by neither.
        inside = next(
            (
                n
                for n in walk(scroll)
                if n is not scroll and n["segment"] is not None
            ),
            None,
        )
        assert inside is not None, "premise: the scroll has addressable content"
        assert_eq(
            inside["path"].count("/"),
            scroll["path"].count("/") + 1,
            "a node one level inside the scroll is one segment past it — the "
            "content between them consumed none",
        )
        assert_subtree(profile(tf, path=inside["path"])["root"], inside, inside["path"])

        # An empty address is the whole window spelled out, not an error.
        for spelling in ("/", "/window[main]/"):
            same = profile(tf, path=spelling)
            assert_eq(
                shape(same["root"]),
                shape(root),
                f"{spelling!r} addresses the scene root — the whole window, said "
                f"explicitly",
            )
            assert_eq(same["nodes_total"], whole["nodes_total"], "over the same census")
            assert_eq(same["path"], spelling, "…with the spelling echoed as sent")

        # The address is the shared vocabulary: the scope resolves through
        # `scene/query` too, so "which subtree is expensive", "profile just that"
        # and "act on it" are one string reaching one node by two derivations.
        ext_row = next(n for n in walk(root) if n["tag"] == LIST_TAG)
        assert_eq(
            tf.request("scene/query", {"path": f"{ext_row['path']}/external/rows"}).result,
            tf.query(f"{EXT}/rows"),
            "a profile row's address resolves through `scene/query` to the model "
            "value the short form reaches — the paint walk and "
            "`Scene::lookup_path_ref` agree on one address",
        )
        assert_eq(
            profile(tf, path=ext_row["path"])["root"]["tag"],
            LIST_TAG,
            "…and that same address roots a profile",
        )

        # ---- (G) scoping survives scale, and the guard can fail -----------------
        list_path = scroll["path"]
        set_rows(tf, 100)
        drive_frame(tf, "the model at 100 rows")
        small = profile(tf, path=list_path)
        set_rows(tf, 1_000_000)
        drive_frame(tf, "the model at 1,000,000 rows")
        big = profile(tf, path=list_path)
        assert_eq(
            shape(big["root"]),
            shape(small["root"]),
            "growing the model four orders of magnitude leaves the SCOPED profile "
            "identical — every path, every kind, every tag. Per-frame work is "
            "bounded by what is visible, stated for one subtree on its own",
        )
        assert_eq(
            costs(big["root"]),
            costs(small["root"]),
            "…and every count inside it, in all six units",
        )
        assert_eq(
            big["nodes_total"],
            small["nodes_total"],
            "…over a measurement that did not grow either",
        )

        set_rows(tf, 100)
        drive_frame(tf, "back to 100 rows")
        tf.intervene(f"{EXT}/eager", True)
        assert_eq(tf.query(f"{EXT}/eager"), True, "the model took the eager arm")
        drive_frame(tf, "the eager arm at 100 rows")
        eager_scroll = next(
            n for n in walk(profile(tf)["root"]) if n["kind"] == "Scroll"
        )["path"]
        eager_small = profile(tf, path=eager_scroll)
        set_rows(tf, 1_000)
        drive_frame(tf, "the eager arm at 1,000 rows")
        eager_big = profile(tf, path=eager_scroll)
        assert eager_big["nodes_total"] > eager_small["nodes_total"] * 5, (
            f"the eager arm builds one node per row, so the SCOPED profile must "
            f"grow with the model ({eager_small['nodes_total']} -> "
            f"{eager_big['nodes_total']}). A guard that only ever measures the "
            f"passing case cannot fail"
        )
        assert eager_big["root"]["total"]["paths"] > eager_small["root"]["total"]["paths"], (
            "…and so must the work it attributes"
        )
        tf.intervene(f"{EXT}/eager", False)
        set_rows(tf, 1_000)
        drive_frame(tf, "back to the virtual arm")

        # ---- (H) every refusal is named ----------------------------------------
        live = profile(tf)["root"]["path"]
        for params, tag in (
            ({"path": "/window[main]/no-such-node"}, "UnknownPath"),
            ({"path": "/window[main/x"}, "MalformedPrefix"),
            ({"path": "/window[nope]/x"}, "UnknownWindow"),
            ({"path": 7}, 'MalformedParam: "path"'),
        ):
            try:
                tf.request("scene/draw_profile", params)
            except RpcError as exc:
                assert tag in str(exc), f"{params} must be refused as {tag}, got {exc}"
                if tag == "UnknownPath":
                    assert "/window[main]/no-such-node" in str(exc), (
                        f"the refusal quotes the address that stopped resolving: {exc}"
                    )
                    assert "DrawProfileUnavailable" not in str(exc), (
                        f"an address that reached nothing is NOT a host with no "
                        f"window — collapsing the two would read a typo as a dead "
                        f"window: {exc}"
                    )
                if tag == "UnknownWindow":
                    assert "main" in str(exc), (
                        f"the refusal teaches the declared window set: {exc}"
                    )
            else:
                raise AssertionError(f"{params} was accepted")

        assert_eq(
            profile(tf)["root"]["path"],
            live,
            "…and every refusal left the profile itself intact",
        )

        methods = {
            m["name"]: m["occ"] for m in tf.request("rpc/methods", {}).result["methods"]
        }
        assert_eq(
            methods.get("scene/draw_profile"),
            "read",
            "the method is still classed as a read — scoping a profile changes no "
            "scene state either",
        )

    # ---- (I) the address decides WHICH WINDOW, by one rule -----------------
    #
    # A second binding, because this is the claim a single-window app cannot
    # test at all: which window a profile measures and which window its rows
    # are addressed to are two answers that must be the same string. Two copies
    # of that expression would give a profile of one window whose every row
    # addresses another — and each of those addresses would resolve, somewhere
    # else, which is the worst kind of wrong.
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        for window in ("main", "inspector"):
            tf.request("scene/screenshot", {"path": "", "window": window})

        by_scope = {w: profile(tf, window=w) for w in ("main", "inspector")}
        assert_eq(
            by_scope["main"]["root"]["path"], "/window[main]/", "main is addressed"
        )
        assert_eq(
            by_scope["inspector"]["root"]["path"],
            "/window[inspector]/",
            "…and the inspector is a different window with its own address space",
        )
        assert shape(by_scope["main"]["root"]) != shape(by_scope["inspector"]["root"]), (
            "premise: the two windows paint different trees, or nothing below "
            "could tell them apart"
        )

        # No `window` param at all: the ADDRESS carries the window, and the
        # embedder re-encoded that one.
        by_path = profile(tf, path="/window[inspector]/")
        assert_eq(
            shape(by_path["root"]),
            shape(by_scope["inspector"]["root"]),
            "an address with an explicit prefix selects the window on its own — "
            "the same window the rows are then addressed against",
        )
        assert_eq(
            costs(by_path["root"]),
            costs(by_scope["inspector"]["root"]),
            "…down to every count in it",
        )
        assert shape(by_path["root"]) != shape(by_scope["main"]["root"]), (
            "…and it is emphatically not the primary window's profile wearing "
            "the inspector's addresses"
        )

        # Scoping inside the second window works exactly as in the first.
        kid = by_path["root"]["children"][0]
        scoped = profile(tf, path=kid["path"])
        assert kid["path"].startswith("/window[inspector]/"), (
            f"a row of the inspector's profile is addressed to it: {kid['path']}"
        )
        assert_subtree(scoped["root"], kid, kid["path"])
        assert scoped["nodes_total"] < by_path["nodes_total"], (
            f"…and scoping there measured less ({scoped['nodes_total']} of "
            f"{by_path['nodes_total']})"
        )

        # Naming two DIFFERENT windows in one request is refused rather than
        # silently resolved in favour of either. Reachable only here: on a
        # single-window binding the disagreeing name is also an undeclared one,
        # and the shell rejects that first, by its own name.
        try:
            tf.request(
                "scene/draw_profile",
                {"path": "/window[inspector]/", "window": "main"},
            )
        except RpcError as exc:
            assert "WindowMismatch" in str(exc), f"expected WindowMismatch, got {exc}"
            assert "inspector" in str(exc) and "main" in str(exc), (
                f"the refusal names both windows it was asked to reconcile: {exc}"
            )
        else:
            raise AssertionError(
                "a request naming two different windows was accepted — one of "
                "them silently won"
            )

        # Agreement is not a conflict.
        agreed = profile(tf, path="/window[inspector]/", window="inspector")
        assert_eq(
            shape(agreed["root"]),
            shape(by_path["root"]),
            "naming the same window twice is one answer, not a refusal",
        )


if __name__ == "__main__":
    run_demo("R1558 the profile is rooted where you ask", main)
