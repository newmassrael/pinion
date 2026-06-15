#!/usr/bin/env python3
"""R946 §5.16 §5.22 §5.27 §2 virtualized lazy outliner — `hello-lazy-tree`.

The World Outliner at scale: a tree that is *both* lazy (children fetched on
expand, R942) *and* virtualized (only the scroll window paints, R819). Expanding
a top-level section loads a folder of hundreds of entries, yet only the
~viewport-sized window of rows ever becomes scene nodes — the off-window rows
never exist as paint nodes until a scroll brings them in. This is the
combination the self-hosted editor's scene / world outliner needs (100k-actor
scenes, streamed and windowed) and the piece the eager windowed tree
(`view_virtual_tree`) cannot do, because a lazy flattening interleaves skeleton
placeholders that a uniform `&[VisibleRow]` cannot carry.

Everything is observed as DATA (§2 #7 scene-as-data; no pixels): the painted
window through `scene/snapshot` (the `{ROW_PREFIX}#id` rows actually realized),
the *whole* structure through the `scene/query` introspection surface (the AI
omniscience a windowed scene cannot give), and the keyboard ⊥ virtualization
scroll-into-view through the scroll node's offset.

ZERO-FLAKE: each child fetch is a deterministic deferred future, every
`scene/snapshot from=paint` advances the pump one step, and every scroll / key
dispatch commits before its response — so `wait_snap` / `wait_until` poll
observed post-action state, never wall-clock (the R942 / R819 discipline). The
assertions are structural (painted-count « full-count, set membership, offset
monotonicity), never timing.

Run from the workspace root:
    cargo build -p hello-lazy-tree --release
    python3 tools/demos/r946_virtual_lazy_tree.py
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
    wait_until,
)

VIEWPORT = (460, 520)
PITCH = 48  # TreeViewStyle::default().row_height — the windowing slot pitch.

ROOT_TAG = "lazytree"
ROW_PREFIX = "lazytree_row"
SKELETON_TAG = "lazytree_skeleton"
STATE_TAG = "lazytree_state"
SCROLL_TAG = "lazytree_scroll"


def row_tag(node_id: str) -> str:
    return f"{ROW_PREFIX}#{node_id}"


def painted_ids(snap) -> list[str]:
    """Every loaded node id with a realized scene node, in flatten order."""
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


def scroll_node(snap):
    return find_by_tag(snap, SCROLL_TAG)


def scroll_offset(snap) -> int:
    node = scroll_node(snap)
    return int((node or {}).get("offset_y", -1))


def scroll_viewport_h(snap) -> int:
    node = scroll_node(snap)
    vp = (node or {}).get("viewport") or {}
    return int(vp.get("h", 0))


def body() -> None:
    with RpcSubprocess("hello-lazy-tree", boot_grace=1.0) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=VIEWPORT)

        def row_count() -> int:
            return tf.query(f"/{STATE_TAG}/external/row_count")

        # ── (A) boot: the 6 sections fit the window (paint == flatten) ───────
        snap = wait_snap(
            tf,
            lambda s: "0" in painted_ids(s),
            viewport=VIEWPORT,
            desc="root children resolve at boot",
        )
        assert_eq(painted_ids(snap), ["0", "1", "2", "3", "4", "5"], "boot: 6 sections painted")
        assert_eq(row_count(), 6, "query row_count == 6 at boot")
        assert_eq(scroll_offset(snap), 0, "boot scroll offset is 0")
        vp_h = scroll_viewport_h(snap)
        assert vp_h > 0, f"AutoSizer measured the flex band height (>0), got {vp_h}"
        assert vp_h < VIEWPORT[1], f"the band is smaller than the window, got {vp_h}"

        # ── (B) expand a LARGE folder → paint window « full flatten ──────────
        tf.click(path=row_tag("0"))
        # The fetch is observable as a skeleton first.
        wait_snap(
            tf,
            lambda s: has_skeleton(s),
            viewport=VIEWPORT,
            desc="expanding the large folder shows a loading skeleton",
        )
        resolved = wait_snap(
            tf,
            lambda s: "0/0" in painted_ids(s) and not has_skeleton(s),
            viewport=VIEWPORT,
            desc="the folder's children resolve",
        )
        full = row_count()
        assert full > 300, f"node 0 is a folder of hundreds of entries ({full} rows)"
        painted = painted_ids(resolved)
        assert len(painted) < 30, f"only the window paints: {len(painted)} of {full} rows"
        assert len(painted) >= 8, f"the window covers the visible band ({len(painted)} rows)"
        # The top of the folder is in the window; a deep child and the sections
        # below it are NOT realized — the virtualization witness.
        assert "0" in painted, "the expanded folder row is at the top of the window"
        assert "0/0" in painted, "the folder's first child is in the window"
        assert "0/300" not in painted, "a deep child is NOT realized (never a scene node)"
        assert "5" not in painted, "sections below the folder are pushed off the window"
        # The whole structure stays visible to an AI through the query surface.
        assert_eq(tf.query(f"/{STATE_TAG}/external/expanded_at.0"), True, "folder is expanded")
        assert_eq(tf.query(f"/{STATE_TAG}/external/id_at.1"), "0/0", "query id_at.1 == first child")
        last = full - 1
        assert_eq(tf.query(f"/{STATE_TAG}/external/id_at.{last}"), "5", "query last row == section 5")

        # ── (C) scroll the window → a DIFFERENT set of rows materializes ─────
        tf.scroll(SCROLL_TAG, to=(0, 200 * PITCH))
        mid = wait_until(
            lambda: (lambda s: s if scroll_offset(s) == 200 * PITCH else None)(snap_now()),
            desc="scroll-to advances the offset to 200*pitch",
        )
        mid_ids = painted_ids(mid)
        assert "0" not in mid_ids, "the folder header scrolled out of the window"
        assert len(mid_ids) < 30, "the window stays small mid-scroll"
        # The mid window is a contiguous run of the folder's children — rows
        # that never existed as nodes before this scroll.
        assert any(i.startswith("0/") for i in mid_ids), "mid-folder children materialized on scroll"
        assert not has_skeleton(mid), "scrolling a loaded folder shows no skeleton"
        assert_eq(row_count(), full, "scrolling does not change the flatten — only what paints")

        # ── (D) scroll to the bottom → the last section is reachable ─────────
        tf.scroll(SCROLL_TAG, to=(0, 10 ** 9))
        bottom = wait_until(
            lambda: (lambda s: s if "5" in painted_ids(s) else None)(snap_now()),
            desc="scroll-to-bottom reaches the last section",
        )
        assert "5" in painted_ids(bottom), "the last section is reachable at the bottom"
        assert "0/0" not in painted_ids(bottom), "the folder's first child scrolled away"
        assert len(painted_ids(bottom)) < 30, "the window is small even at the bottom"

        # ── (E) scroll back to the top → the boot-of-expand window returns ───
        tf.scroll(SCROLL_TAG, to=(0, 0))
        top = wait_until(
            lambda: (lambda s: s if scroll_offset(s) == 0 else None)(snap_now()),
            desc="scrolled back to the top",
        )
        assert "0" in painted_ids(top), "the folder header is back in the top window"
        assert "0/0" in painted_ids(top), "the folder's first child is back"

        # ── (F) keyboard ⊥ virtualization: roving reveals off-window rows ────
        tf.request("focus/set", {"tag": ROOT_TAG})
        # Land the cursor, then drive it well past the bottom of the window.
        tf.key(path=ROOT_TAG, name="ArrowDown")  # cursor onto row 0
        wait_until(
            lambda: tf.query(f"/{STATE_TAG}/external/cursor") == "0",
            desc="ArrowDown lands the cursor on the folder row",
        )
        assert_eq(scroll_offset(snap_now()), 0, "the top row needs no scroll")
        for _ in range(40):
            tf.key(path=ROOT_TAG, name="ArrowDown")  # descend deep into the folder
        revealed = wait_until(
            lambda: (lambda s: s if scroll_offset(s) > 0 else None)(snap_now()),
            desc="navigating down scrolled the cursor's row into view",
        )
        # The cursor row is realized (painted) — the reveal kept it in the window.
        cursor = tf.query(f"/{STATE_TAG}/external/cursor")
        assert cursor is not None and cursor.startswith("0/"), f"cursor descended into the folder: {cursor}"
        assert cursor in painted_ids(revealed), "the cursor row was scrolled into the window (realized)"


if __name__ == "__main__":
    sys.exit(run_demo("r946_virtual_lazy_tree", body))
