#!/usr/bin/env python3
"""R942 §5.22 §5.23 §5.27 §2 lazy-loaded scene outliner — `hello-lazy-tree`.

A tree whose **children are fetched asynchronously on expand**: each branch's
children arrive through the §5.22 `Resource` carrier (a per-node
`ResourceCache`), an `Effect` on the expand-set Signal drives the fetches
through the shell-polled `LocalTaskPump`, and a freshly-expanded branch renders
a skeleton placeholder until its children resolve. This is the same async
substrate R923/R924 introduced (asset browser / lazy list), now keyed on expand
over a tree instead of scroll over a flat list. The structure itself is
out-of-memory; only expanded branches are ever fetched — all observed as DATA
through `scene/snapshot` + `scene/query` (§2 #7 scene-as-data; no pixels).

ZERO-FLAKE latency model: each child fetch is a deterministic deferred future
(`Pending` N times → resolve), and every `scene/snapshot from=paint` advances
the pump one step. So the demo's own snapshot polling drives `skeleton → rows`
— `wait_snap` on the skeleton is guaranteed to catch it before the children
resolve (no wall-clock race; same discipline as the R923 deferred demo). The
skeleton is demonstrated on an *expand* action (where the demo controls when
the fetch starts), not at boot (whose paint count is environment-dependent).

Run from the workspace root:
    cargo build -p hello-lazy-tree --release
    python3 tools/demos/r942_lazy_tree.py
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
    wait_snap,
)

VIEWPORT = (460, 520)

ROW_PREFIX = "lazytree_row"
STATUS_TAG = "lazytree_status"
SKELETON_TAG = "lazytree_skeleton"
STATE_TAG = "lazytree_state"

ELLIPSIS = "…"  # …


def row_tag(node_id: str) -> str:
    return f"{ROW_PREFIX}#{node_id}"


def visible_ids(snap) -> list[str]:
    """Every loaded node id painted in the outliner, in flatten order."""
    out: list[str] = []

    def walk(node) -> None:
        if not isinstance(node, dict):
            return
        tag = node.get("tag")
        if isinstance(tag, str) and tag.startswith(ROW_PREFIX + "#"):
            out.append(tag[len(ROW_PREFIX) + 1 :])
        for child in node.get("children", []) or []:
            walk(child)
        content = node.get("content")
        if isinstance(content, dict):
            walk(content)

    walk(snap)
    return out


def has_skeleton(snap) -> bool:
    return find_by_tag(snap, SKELETON_TAG) is not None


def text_under(snap, tag: str):
    node = find_by_tag(snap, tag)
    if node is None:
        return None

    def first_text(n):
        if not isinstance(n, dict):
            return None
        if n.get("type") == "Text":
            return n.get("content")
        for child in n.get("children", []) or []:
            hit = first_text(child)
            if hit is not None:
                return hit
        return None

    return first_text(node)


def status_of(snap):
    return text_under(snap, STATUS_TAG)


def label_under(snap, node_id: str):
    """The label TextNode painted in the row for `node_id` (the last Text in
    its row container; the leading glyph is presentational)."""
    node = find_by_tag(snap, row_tag(node_id))
    if node is None:
        return None
    texts = []

    def collect(n) -> None:
        if not isinstance(n, dict):
            return
        if n.get("type") == "Text":
            texts.append(n.get("content"))
        for child in n.get("children", []) or []:
            collect(child)

    collect(node)
    return texts[-1] if texts else None


def body() -> None:
    with RpcSubprocess("hello-lazy-tree", boot_grace=1.0) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=VIEWPORT)

        def row_count() -> int:
            return tf.query(f"/{STATE_TAG}/external/row_count")

        # ── (A) boot: the root's children load lazily, then 6 top-level rows ──
        # The root fetch resolves under the demo's own snapshot polling. R946 —
        # at boot the 6 sections all fit the window, so the painted ids == the
        # full flatten; once a folder expands (B) the two diverge.
        snap = wait_snap(
            tf,
            lambda s: "0" in visible_ids(s),
            viewport=VIEWPORT,
            desc="root children resolve at boot (node 0 painted)",
        )
        ids0 = visible_ids(snap)
        assert_eq(ids0, ["0", "1", "2", "3", "4", "5"], "boot: 6 top-level rows in order")
        assert not has_skeleton(snap), "boot: no skeleton once the root resolved"
        assert_eq(status_of(snap), "6 items loaded", "boot status line")
        assert find_by_tag(snap, "lazytree") is not None, "outliner root present at boot"

        # Top-level labels are the deterministic synthetic fixture.
        assert_eq(label_under(snap, "0"), "Scenes", "node 0 label")
        assert_eq(label_under(snap, "1"), "albedo_1.fbx", "node 1 label (leaf)")
        assert_eq(label_under(snap, "2"), "Textures", "node 2 label")

        # Collapsed at boot: no descendant rows painted.
        assert not any(i.startswith("0/") for i in ids0), "node 0 collapsed at boot"
        assert not any(i.startswith("2/") for i in ids0), "node 2 collapsed at boot"

        # (A1) the AI-first tree-state introspection surface mirrors the paint.
        assert_eq(row_count(), 6, "row_count at boot")
        assert_eq(tf.query(f"/{STATE_TAG}/external/id_at.0"), "0", "id_at.0 at boot")
        assert_eq(tf.query(f"/{STATE_TAG}/external/label_at.2"), "Textures", "label_at.2")
        assert_eq(tf.query(f"/{STATE_TAG}/external/level_at.0"), 1, "level_at.0 (root depth)")
        assert_eq(
            tf.query(f"/{STATE_TAG}/external/expanded_at.0"),
            False,
            "node 0 is a collapsed branch",
        )
        assert_eq(
            tf.query(f"/{STATE_TAG}/external/expanded_at.1"),
            None,
            "node 1 is a leaf (aria-expanded undefined)",
        )

        # ── (B) expand node 0 (a LARGE folder) → skeleton, then children ─────
        tf.click(path=row_tag("0"))
        # (B1) the loading state is deterministically observable: the fetch
        # needs many polls, so the first snapshots after the click show a
        # skeleton + the "Loading…" status band.
        loading = wait_snap(
            tf,
            lambda s: has_skeleton(s) and (status_of(s) or "").startswith("Loading"),
            viewport=VIEWPORT,
            desc="expanding node 0 shows a skeleton while its children fetch",
        )
        assert_eq(status_of(loading), f"Loading Scenes{ELLIPSIS}", "loading status names the branch")
        assert not any(
            i.startswith("0/") for i in visible_ids(loading)
        ), "no children visible until the fetch resolves"

        # (B2) the children resolve under continued polling.
        resolved = wait_snap(
            tf,
            lambda s: "0/0" in visible_ids(s),
            viewport=VIEWPORT,
            desc="node 0's children appear after the lazy fetch",
        )
        assert not has_skeleton(resolved), "children resolved → skeleton gone"
        # (B2a) R946 — node 0 is a folder of HUNDREDS of entries, but only the
        # ~viewport-sized window paints. The full flatten lives in the query
        # surface; the painted scene is a small window over it.
        big = row_count()
        assert big > 300, f"node 0 is a large folder ({big} rows after expand)"
        painted = visible_ids(resolved)
        assert len(painted) < 30, f"only the window paints ({len(painted)} of {big} rows)"
        assert "0/0" in painted, "the folder's first child is in the top window"
        assert "5" not in painted, "rows below the window (e.g. section 5) are not painted"
        assert_eq(label_under(resolved, "0/0"), "Scenes", "child 0/0 label")
        assert_eq(label_under(resolved, "0/1"), "albedo_1.fbx", "child 0/1 label (leaf)")

        # (B3) introspection reflects the deeper tree (level = depth + 1), the
        # whole flatten regardless of which window paints.
        assert_eq(tf.query(f"/{STATE_TAG}/external/expanded_at.0"), True, "node 0 now expanded")
        assert_eq(tf.query(f"/{STATE_TAG}/external/id_at.1"), "0/0", "id_at.1 == first child")
        assert_eq(tf.query(f"/{STATE_TAG}/external/level_at.1"), 2, "child level == 2")

        # ── (C) collapse node 0 → children hidden, no skeleton, cache kept ───
        tf.click(path=row_tag("0"))
        collapsed = wait_snap(
            tf,
            lambda s: not any(i.startswith("0/") for i in visible_ids(s)),
            viewport=VIEWPORT,
            desc="collapse hides node 0's children",
        )
        assert not has_skeleton(collapsed), "collapse is synchronous — no skeleton"
        assert_eq(visible_ids(collapsed), ["0", "1", "2", "3", "4", "5"], "back to 6 top rows")
        assert_eq(row_count(), 6, "row_count after collapse")

        # ── (D) re-expand node 0 → cache hit: children return with NO skeleton ─
        tf.click(path=row_tag("0"))
        recached = wait_snap(
            tf,
            lambda s: "0/0" in visible_ids(s),
            viewport=VIEWPORT,
            desc="re-expanding a cached branch returns its children",
        )
        assert not has_skeleton(recached), (
            "re-expanding a cached branch shows NO skeleton (no re-fetch) — the "
            "retained child set resolves the same frame"
        )
        assert row_count() > 300, "cached folder restored to its full row count"
        assert "0/0" in visible_ids(recached), "cached first child back in the window"

        # ── (E) expand a deeper branch (0/0) → multi-level lazy load ──────────
        # 0/0 is the second row, inside the top window, so it is clickable.
        tf.click(path=row_tag("0/0"))
        deep_loading = wait_snap(
            tf,
            lambda s: has_skeleton(s),
            viewport=VIEWPORT,
            desc="expanding the deeper branch 0/0 fetches its children",
        )
        assert (status_of(deep_loading) or "").startswith("Loading"), "deep expand shows loading"
        deep = wait_snap(
            tf,
            lambda s: "0/0/0" in visible_ids(s),
            viewport=VIEWPORT,
            desc="0/0's children resolve (a grandchild row appears)",
        )
        assert not has_skeleton(deep), "deep children resolved → skeleton gone"
        deep_ids = visible_ids(deep)
        assert "0/0/0" in deep_ids, "grandchild 0/0/0 visible"
        assert deep_ids.index("0/0/0") == deep_ids.index("0/0") + 1, "grandchild follows its parent"
        # 0/0/0 is at depth 2 → aria-level 3 (query the full-flatten position).
        assert_eq(tf.query(f"/{STATE_TAG}/external/level_at.2"), 3, "grandchild level == 3")

        # ── (F) clicking a leaf is a no-op (the reducer gates on id_is_branch) ─
        before_leaf = visible_ids(deep)
        # 0/1 is a leaf inside the top window (odd sibling index → not a branch).
        tf.click(path=row_tag("0/1"))
        # Dispatch commits before the response, so the unchanged set is readable
        # directly (no async edge to gate on).
        after = snap_now()
        assert_eq(visible_ids(after), before_leaf, "clicking a leaf changes nothing")
        assert not has_skeleton(after), "clicking a leaf starts no fetch (no skeleton)"


if __name__ == "__main__":
    sys.exit(run_demo("r942_lazy_tree", body))
