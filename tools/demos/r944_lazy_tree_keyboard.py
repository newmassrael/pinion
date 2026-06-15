#!/usr/bin/env python3
"""R944 §5.22 §5.27 §2 keyboard navigation for the lazy outliner — `hello-lazy-tree`.

R942 gave the lazy-loaded scene outliner its pointer + async-fetch model; R944
makes it keyboard-navigable (WAI-ARIA APG 6.13 Tree). While the tree root holds
focus the arrow keys rove a keyboard cursor over the loaded rows through the
pure `resolve_tree_key` resolver (the third external-overlay consumer of the
`tree_nav` substrate). The one axis new to a *lazy* tree: an Arrow Right that
expands a not-yet-loaded branch routes through the same expand-set a click does,
so the existing fetch-on-expand `Effect` fires — keyboard expansion lazy-loads
exactly as clicking does.

Everything is observed as DATA: the cursor through the `scene/query` `cursor` /
`cursor_index` introspection surface, the visible focus highlight through the
M3 state-layer fill in `scene/snapshot` (§2 #7 scene-as-data; no pixels). The
focus gate (routing ⊥ focus) is exercised end-to-end: keys before `focus/set`
do not navigate.

ZERO-FLAKE: every RPC dispatch commits before its response, and every
`scene/snapshot from=paint` advances the deterministic fetch pump one step, so
`wait_query` / `wait_snap` / `wait_until` poll the observed post-action state —
no wall-clock race (same discipline as the R942 demo).

Run from the workspace root:
    cargo build -p hello-lazy-tree --release
    python3 tools/demos/r944_lazy_tree_keyboard.py
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
    wait_query,
    wait_snap,
    wait_until,
)

VIEWPORT = (460, 520)

ROOT_TAG = "lazytree"
ROW_PREFIX = "lazytree_row"
SKELETON_TAG = "lazytree_skeleton"
STATE_TAG = "lazytree_state"


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


def focused_row(snap) -> str | None:
    """The row tag carrying the opaque M3 focus state-layer fill — the
    keyboard cursor. `row_focus_bg` fills the cursor row with an opaque
    colour (a == 255); every other row is transparent."""
    found: list[str] = []

    def walk(node) -> None:
        if not isinstance(node, dict):
            return
        tag = node.get("tag")
        if isinstance(tag, str) and tag.startswith(ROW_PREFIX + "#"):
            fill = (node.get("style") or {}).get("fill") or {}
            if fill.get("a", 0) == 255:
                found.append(tag)
        for child in node.get("children", []) or []:
            walk(child)
        content = node.get("content")
        if isinstance(content, dict):
            walk(content)

    walk(snap)
    return found[0] if found else None


def body() -> None:
    with RpcSubprocess("hello-lazy-tree", boot_grace=1.0) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=VIEWPORT)

        def cursor_q():
            return tf.query(f"/{STATE_TAG}/external/cursor")

        def press(name: str) -> None:
            tf.key(path=ROOT_TAG, name=name)

        def expect_cursor(node_id, label) -> None:
            """Wait for the cursor to land on `node_id`, then assert it is
            reflected both in the AI-first `scene/query` surface and in the
            painted focus highlight (query ⟷ paint never diverge)."""
            expected_tag = None if node_id is None else row_tag(node_id)
            wait_until(lambda: cursor_q() == node_id, desc=label)
            assert_eq(cursor_q(), node_id, f"{label} (query cursor)")
            wait_until(lambda: focused_row(snap_now()) == expected_tag, desc=label)
            assert_eq(focused_row(snap_now()), expected_tag, f"{label} (paint highlight)")

        # ── (A) boot: the root's children load lazily, no cursor yet ─────────
        snap = wait_snap(
            tf,
            lambda s: "0" in visible_ids(s),
            viewport=VIEWPORT,
            desc="root children resolve at boot",
        )
        assert_eq(visible_ids(snap), ["0", "1", "2"], "boot: 3 top-level rows")
        assert_eq(cursor_q(), None, "no keyboard cursor at boot")
        assert_eq(tf.query(f"/{STATE_TAG}/external/cursor_index"), None, "cursor_index unset at boot")
        assert_eq(focused_row(snap), None, "no focus highlight at boot")

        # ── (B) focus gate — keys before focus/set do NOT navigate ───────────
        press("ArrowDown")
        press("End")
        # The tree root does not hold focus yet, so the keys are not consumed
        # by the tree → the cursor is untouched (routing ⊥ focus).
        assert_eq(cursor_q(), None, "keys before focus/set do not navigate")
        assert_eq(focused_row(snap_now()), None, "no highlight before focus/set")

        tf.request("focus/set", {"tag": ROOT_TAG})

        # ── (C) Arrow Down from an empty cursor lands on the first row ───────
        press("ArrowDown")
        expect_cursor("0", "ArrowDown after focus lands on the first row")
        assert_eq(tf.query(f"/{STATE_TAG}/external/cursor_index"), 0, "cursor_index 0")

        # ── (D) vertical nav: step / clamp / Home / End ──────────────────────
        press("ArrowDown")
        expect_cursor("1", "ArrowDown steps to the next row")
        press("ArrowUp")
        expect_cursor("0", "ArrowUp steps back")
        press("ArrowUp")  # clamp at top (no wrap)
        assert_eq(cursor_q(), "0", "ArrowUp clamps at the first row")
        press("End")
        expect_cursor("2", "End jumps to the last row")
        press("ArrowDown")  # clamp at bottom (no wrap)
        assert_eq(cursor_q(), "2", "ArrowDown clamps at the last row")
        press("Home")
        expect_cursor("0", "Home jumps to the first row")

        # ── (E) Arrow Right on a collapsed branch lazy-loads its children ────
        press("ArrowRight")
        loading = wait_snap(
            tf,
            lambda s: has_skeleton(s),
            viewport=VIEWPORT,
            desc="keyboard expand of node 0 shows the loading skeleton",
        )
        assert not any(
            i.startswith("0/") for i in visible_ids(loading)
        ), "no children visible until the keyboard-triggered fetch resolves"
        wait_query(
            tf,
            f"/{STATE_TAG}/external/expanded_at.0",
            True,
            desc="Arrow Right expanded the branch (keyboard drives the expand set)",
        )
        assert_eq(cursor_q(), "0", "cursor stays on the branch while it loads")

        resolved = wait_snap(
            tf,
            lambda s: "0/0" in visible_ids(s),
            viewport=VIEWPORT,
            desc="node 0's children appear after the lazy fetch",
        )
        assert not has_skeleton(resolved), "children resolved → skeleton gone"
        assert_eq(
            visible_ids(resolved),
            ["0", "0/0", "0/1", "1", "2"],
            "keyboard expand inserted the children",
        )

        # ── (F) Arrow Right again descends to the first child ────────────────
        press("ArrowRight")
        expect_cursor("0/0", "Arrow Right on the expanded branch descends to its first child")

        # ── (G) Arrow Left ascends from a collapsed child, then collapses ────
        press("ArrowLeft")  # 0/0 is a collapsed branch → ascend to parent
        expect_cursor("0", "Arrow Left on a collapsed branch ascends to the parent")
        press("ArrowLeft")  # parent is expanded → collapse it
        collapsed = wait_snap(
            tf,
            lambda s: not any(i.startswith("0/") for i in visible_ids(s)),
            viewport=VIEWPORT,
            desc="Arrow Left collapses the expanded branch",
        )
        assert_eq(visible_ids(collapsed), ["0", "1", "2"], "collapsed back to the top level")
        assert not has_skeleton(collapsed), "collapse is synchronous — no skeleton"
        assert_eq(cursor_q(), "0", "cursor stays on the collapsed branch")

        # ── (H) Enter / Space toggle the cursor branch ───────────────────────
        press("Enter")  # toggle → expand (cache hit, no skeleton)
        recached = wait_snap(
            tf,
            lambda s: "0/0" in visible_ids(s),
            viewport=VIEWPORT,
            desc="Enter expands the branch (cached children return)",
        )
        assert not has_skeleton(recached), "Enter re-expand is a cache hit (no re-fetch)"
        press("Space")  # toggle → collapse
        wait_query(
            tf,
            f"/{STATE_TAG}/external/expanded_at.0",
            False,
            desc="Space collapses the branch",
        )

        # ── (I) cursor reanchors when a click hides its row ──────────────────
        # Re-expand and descend so the cursor sits on a child, then collapse
        # the ancestor via the click path: the cursor's row vanishes, so the
        # gated cursor (query + highlight) must drop to nothing — never a
        # dangling aria-activedescendant / orphaned highlight (R943.1 class).
        press("Enter")
        wait_snap(tf, lambda s: "0/0" in visible_ids(s), viewport=VIEWPORT, desc="re-expand for reanchor")
        press("ArrowRight")
        expect_cursor("0/0", "cursor on a child before the collapse")
        tf.click(path=row_tag("0"))  # click-path collapse of the ancestor
        hidden = wait_snap(
            tf,
            lambda s: not any(i.startswith("0/") for i in visible_ids(s)),
            viewport=VIEWPORT,
            desc="clicking the ancestor collapses it, hiding the cursor's row",
        )
        assert_eq(cursor_q(), None, "gated cursor reanchors to none (no dangling descendant)")
        assert_eq(focused_row(hidden), None, "no orphaned focus highlight after the cursor row hides")

        # ── (J) the cursor self-heals: the next vertical key restarts at row 0 ─
        press("ArrowDown")
        expect_cursor("0", "the next Arrow Down restarts navigation at the first row")


if __name__ == "__main__":
    sys.exit(run_demo("r944_lazy_tree_keyboard", body))
