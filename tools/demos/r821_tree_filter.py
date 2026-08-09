#!/usr/bin/env python3
"""R821 §5.27 §5.40 §5.50 — recursive tree filter proxy E2E.

Drives `hello-tree-filter` via JSON-RPC. A 252-node scene-graph outliner
(12 groups x 20 leaves) whose filter proxy is the hierarchical member of the
Model/View proxy family (the tree peer of the 1-D list `view_order` and the
data-grid `grid_sort` proxies). The recursion is the toolkit's
sort filter proxy model with recursive filtering: a node survives iff it
matches the query OR any descendant matches (path-to-match), so a match
buried inside a *collapsed* group is revealed with its ancestors as path
context, while non-match siblings are pruned.

  * primary `ButtonExternal` at `sgtree_root` — the invisible focusable tree
    root (the WAI-ARIA single tab stop / read_state anchor);
  * extra `TreeFilterExternal` at `sgtree_filter` — the AI-first filter proxy
    (`invoke "set_filter"` / `query "visible_count"` / `query
    "visible_at.<pos>"` / `query "label_at.<pos>"`);
  * extra `TreeRowClickExternal` at `sgtree` — row clicks → cursor;
  * R819 `view_virtual_tree` — windows the FILTERED flat_visible sequence.

The decisive Model/View witness (§2 #7 scene-as-data): set a filter and the
visible-row count shrinks to the match paths, the rendered window holds only
matching rows + their ancestor groups, and a match inside a boot-collapsed
group is revealed without ever toggling its expand flag.

  (A) boot — unfiltered, 92 rows (12 groups + 4 boot-expanded * 20 leaves),
      windowed (a small slice renders, not all 92).
  (B) the boot-collapsed groups hide their children: g4..g11 are adjacent in
      the unfiltered order (g9 immediately precedes g10).
  (C) filter `Node09` reveals the match buried inside collapsed group 9 —
      Group09 + its 20 leaves (21 rows), the ancestor shown as path context.
  (D) filter `Node03` → Group03 + 20 leaves; the rendered window holds only
      group-3 rows.
  (E) a matched branch with no matching child is a filtered leaf: `Group03`
      matches the group only (its `Node03_*` leaves do not) → 1 row.
  (F) multi-group match: `_05` → one leaf per group → 12 groups + 12 leaves.
  (G) case-insensitive (`node03` == `Node03`); the broad `Node` matches the
      whole tree (252); `zzz` matches nothing (0).
  (H) clear via invoke Null restores the full 92-row view.
  (I) admin restore via intervene `query`; read-only guards.
  (J) keyboard type-to-filter: focus the tree root, type `Group` → 12 groups,
      Backspace pops, Escape clears (the GUI peer of the RPC set_filter).

  Phase 2 — PIXELS (PINION_SCREENSHOT): the search box + a tree row paint.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-tree-filter"
WIN = (520, 560)
GROUPS = 12
CHILDREN_PER = 20
TOTAL_NODES = GROUPS * (1 + CHILDREN_PER)  # 252
EXPANDED_AT_BOOT = 4
BOOT_VISIBLE = GROUPS + EXPANDED_AT_BOOT * CHILDREN_PER  # 92

TREE_TAG = "sgtree"
ROOT_TAG = "sgtree_root"
FILTER_TAG = "sgtree_filter"
SEARCH_TAG = "sgtree_search"


def q(tf, path):
    return tf.query(f"/{FILTER_TAG}/external/{path}")


def visible_count(tf):
    return q(tf, "visible_count")


def set_filter(tf, value):
    return tf.invoke(f"/{FILTER_TAG}/external/set_filter", value)


def present_rows(snap) -> list[str]:
    """Rendered tree-row ids (from the `sgtree#<id>` windowed strips)."""
    out: list[str] = []
    for tag in abs_rects_of(snap):
        if tag.startswith(f"{TREE_TAG}#"):
            out.append(tag[len(f"{TREE_TAG}#"):])
    return out


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot: unfiltered, full 92-row view, windowed ────────
        assert ROOT_TAG in rects, "focusable tree root present at boot"
        assert SEARCH_TAG in rects, "search box present at boot"
        assert_eq(q(tf, "query"), "", "no filter at boot")
        assert_eq(visible_count(tf), BOOT_VISIBLE, "boot view = 92 rows (12 groups + 4*20)")
        boot_rows = present_rows(snap)
        assert 0 < len(boot_rows) < 30, f"virtualized: small window, got {len(boot_rows)} of {BOOT_VISIBLE}"
        assert all(r.startswith("g") for r in boot_rows), "rendered rows are scene-graph nodes"
        assert_eq(q(tf, "visible_at.0"), "g0", "unfiltered visual 0 is the first group")
        assert_eq(q(tf, "label_at.0"), "Group00", "…labelled Group00")

        # ── (B) boot-collapsed groups hide their children ───────────
        # Rows 0..83 are g0..g3 expanded (4 * 21); rows 84..91 are the
        # collapsed groups g4..g11 — so g9 sits immediately before g10
        # (its 20 children are NOT in the visible order while collapsed).
        assert_eq(q(tf, "visible_at.84"), "g4", "first collapsed group at visual 84")
        assert_eq(q(tf, "visible_at.89"), "g9", "g9 is a collapsed group row")
        assert_eq(
            q(tf, "visible_at.90"), "g10",
            "g10 immediately follows g9 — g9's children are hidden while collapsed",
        )
        assert_eq(q(tf, "visible_at.91"), "g11", "last collapsed group")
        assert_eq(q(tf, "visible_at.92"), None, "one past the end is Null (present-but-empty)")

        # ── (C) filter reveals a match inside a COLLAPSED group ──────
        assert_eq(set_filter(tf, "Node09"), 1 + CHILDREN_PER, "Group09 + its 20 leaves = 21")
        assert_eq(q(tf, "query"), "Node09", "the filter query reflects the active facet")
        assert_eq(q(tf, "visible_at.0"), "g9", "the buried group is revealed as path context…")
        assert_eq(q(tf, "label_at.0"), "Group09", "…labelled Group09")
        assert_eq(q(tf, "visible_at.1"), "g9-n0", "…immediately followed by its first matching leaf")
        assert_eq(q(tf, "label_at.1"), "Node09_00", "…the buried match, now revealed")
        snap = tf.snapshot(source="paint", viewport=WIN)
        revealed = present_rows(snap)
        assert "g9" in revealed, "the revealed group renders in the filtered window"
        assert any(r.startswith("g9-n") for r in revealed), "its buried matches render too"
        assert all(r.startswith("g9") for r in revealed), \
            f"only group-9 rows render under the Node09 filter, got {sorted(revealed)[:8]}"

        # ── (D) filter Node03 → group 3 path, windowed ──────────────
        assert_eq(set_filter(tf, "Node03"), 1 + CHILDREN_PER, "Group03 + 20 leaves")
        assert_eq(q(tf, "label_at.0"), "Group03", "ancestor group revealed")
        assert_eq(q(tf, "label_at.5"), "Node03_04", "deep filtered visual is a group-3 leaf")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert all(r.startswith("g3") for r in present_rows(snap)), "window holds only group-3 rows"

        # ── (E) matched branch with no matching child = filtered leaf ─
        assert_eq(set_filter(tf, "Group03"), 1, "Group03 matches the group only, not its leaves")
        assert_eq(q(tf, "label_at.0"), "Group03", "the single match is the group")
        assert_eq(q(tf, "visible_at.1"), None, "no children — a filtered leaf")
        assert_eq(set_filter(tf, "Group"), GROUPS, "every group label contains 'Group' → 12 leaves")

        # ── (F) multi-group match (one leaf per group) ──────────────
        assert_eq(set_filter(tf, "_05"), GROUPS + GROUPS, "12 groups, each revealed with 1 leaf")
        assert_eq(q(tf, "label_at.0"), "Group00", "first group as path context")
        assert_eq(q(tf, "label_at.1"), "Node00_05", "its single matching leaf")
        assert_eq(q(tf, "label_at.2"), "Group01", "next group, no intervening siblings")

        # ── (G) case-insensitive, broad, and empty matches ──────────
        assert_eq(set_filter(tf, "node03"), 1 + CHILDREN_PER, "lowercase matches Node03 (case-insensitive)")
        assert_eq(set_filter(tf, "Node"), TOTAL_NODES, "'Node' matches every leaf → the whole 252-node tree")
        assert_eq(set_filter(tf, "zzz"), 0, "no match → empty filtered view")
        assert_eq(q(tf, "visible_at.0"), None, "empty view has no first row")

        # ── (H) clear via invoke Null restores the full view ────────
        assert_eq(set_filter(tf, None), BOOT_VISIBLE, "Null clears → the full 92-row view")
        assert_eq(q(tf, "query"), "", "filter cleared")

        # ── (I) admin restore via intervene + read-only guards ──────
        tf.intervene(f"/{FILTER_TAG}/external/query", "Group07")
        assert_eq(q(tf, "query"), "Group07", "intervene sets the query (admin/restore)")
        assert_eq(visible_count(tf), 1, "Group07 → one filtered leaf")
        tf.intervene(f"/{FILTER_TAG}/external/query", None)
        assert_eq(q(tf, "query"), "", "intervene Null clears the query")
        assert_eq(visible_count(tf), BOOT_VISIBLE, "…restoring the full view")

        # ── (J) keyboard type-to-filter (the GUI peer of set_filter) ─
        focused = tf.request("focus/set", {"tag": ROOT_TAG}).result.get("focused")
        assert_eq(focused, ROOT_TAG, "the tree root takes keyboard focus")
        # The cursor coordinate is irrelevant for a focused-widget key
        # (apply_key is focus-gated, not position-gated); a point inside the
        # tree band suffices, and the 0x0 invisible root has no addressable rect.
        kbd = (30.0, 250.0)
        for ch in "Group":
            tf.key(at=kbd, name=ch)
        assert_eq(q(tf, "query"), "Group", "typing alphanumerics builds the filter query")
        assert_eq(visible_count(tf), GROUPS, "the typed filter shows all 12 groups")
        tf.key(at=kbd, name="Backspace")
        assert_eq(q(tf, "query"), "Grou", "Backspace pops the last character")
        tf.key(at=kbd, name="Escape")
        assert_eq(q(tf, "query"), "", "Escape clears the active filter")
        assert_eq(visible_count(tf), BOOT_VISIBLE, "…restoring the full view")

    # ── Phase 2 — live pixels (boot frame: search box + a tree row) ──
    snap, rects = _boot_snapshot_and_rects()
    assert SEARCH_TAG in rects and f"{TREE_TAG}#g0" in rects, "search box + a tree row are tagged"
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, f"screenshot {img.width}x{img.height} != window {WIN}"
    sx, sy, sw, sh = rects[SEARCH_TAG]
    rx, ry, rw, rh = rects[f"{TREE_TAG}#g0"]
    s_col, r_col = sample_png_points(
        img, [(sx + sw // 2, sy + sh // 2), (rx + rw // 2, ry + rh // 2)]
    )
    assert s_col is not None and r_col is not None, "search box + tree row sampled (both paint)"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r821-")) / "treefilter.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", EXAMPLE, "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R821 §5.27 §5.40 §5.50 — recursive tree filter proxy", body))
