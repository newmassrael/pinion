#!/usr/bin/env python3
"""R905 §5.27 §5.52 — scene-outliner batch operations on the multi-selection.

R902 gave the outliner a stable-id multi-selection; R905 makes that selection
actionable: delete-selected and rename-selected, each landing as ONE undo step
so a single undo reverses the whole batch. Both are AI-first invoke ops on the
multi-select coordinator (the §2 RPC primary path), the editor gesture a
scene-outliner needs.

This demo drives it over RPC and verifies, all without OCR:

1. **Batch delete** — selecting visible leaves and deleting removes exactly those
   subtrees (the visible row count drops, the ids vanish), clears the selection,
   and is ONE undo step (a single undo restores them; redo re-applies).
2. **Top-level dedup** — selecting a folder AND one of its children deletes the
   folder subtree ONCE (the child rides along inside it), so the count is 1 and
   the child returns with the folder on undo.
3. **Batch rename** — a `{n}` template numbers the selected nodes in pre-order;
   a literal template renames them all the same; both are ONE undo step.
4. **No-op safety** — deleting/renaming an empty or unchanged selection records
   no history.
5. **Live-pixel** — a `PINION_SCREENSHOT` boot frame rasterises the outliner.

Run from the workspace root:
    cargo build -p hello-tree-grid --release
    python3 tools/demos/r905_batch_ops.py
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    read_png_rgba8,
    png_pixel,
    run_demo,
    wait_until,
)

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
EXAMPLE = "hello-tree-grid"
WIN = (480, 460)
FOLDERS = 24
OBJECTS_PER = 12
EXPANDED_AT_BOOT = 3
BOOT_ROWS = FOLDERS + EXPANDED_AT_BOOT * OBJECTS_PER  # 60
TREE_TAG = "tgrid"
STATE_TAG = "tgrid_state"
SELECT_TAG = "tgrid_select"


def sel(slot: str) -> str:
    return f"/{SELECT_TAG}/external/{slot}"


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r905-")) / "outliner.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-tree-grid"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-tree-grid", "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(f"PINION_SCREENSHOT exited {res.returncode}: {res.stderr!r}")
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        def rows() -> int:
            return int(tf.query(f"/{STATE_TAG}/external/row_count"))

        def visible_ids() -> list[str]:
            return [tf.query(f"/{STATE_TAG}/external/id_at.{i}") for i in range(rows())]

        def selection() -> list:
            return tf.query(sel("selection"))

        def label_at(rid: str) -> str | None:
            # Read a row's label by its visible position (the robust AT/AI path —
            # `label_at` is a peer of `id_at` on the tree-state introspection).
            ids = visible_ids()
            if rid not in ids:
                return None
            return tf.query(f"/{STATE_TAG}/external/label_at.{ids.index(rid)}")

        # ── boot baseline ────────────────────────────────────────────────
        assert_eq(rows(), BOOT_ROWS, "boot visible row count")
        assert_eq(selection(), ["f0"], "boot selection = {f0}")

        # ── (A) batch-delete visible leaves; one undo step ───────────────
        tf.invoke(sel("select"), "f0-o0")
        tf.invoke(sel("toggle"), "f0-o1")
        wait_until(lambda: selection() == ["f0-o0", "f0-o1"], desc="two leaves selected")
        assert_eq(tf.invoke(sel("delete_selected"), None), 2, "two subtrees deleted")
        wait_until(lambda: rows() == BOOT_ROWS - 2, desc="two rows removed")
        assert_eq(selection(), [], "selection cleared after delete")
        assert "f0-o0" not in visible_ids(), "deleted leaf is gone"
        assert_eq(tf.invoke(sel("undo"), None), True, "undo steps")
        wait_until(lambda: rows() == BOOT_ROWS, desc="undo restored the rows")
        assert "f0-o0" in visible_ids(), "undeleted leaf is back"
        assert_eq(tf.invoke(sel("redo"), None), True, "redo re-applies")
        wait_until(lambda: rows() == BOOT_ROWS - 2, desc="redo deletes again")
        assert_eq(tf.invoke(sel("redo"), None), False, "redo at top is a no-op")
        tf.invoke(sel("undo"), None)  # restore baseline
        wait_until(lambda: rows() == BOOT_ROWS, desc="back to baseline")

        # ── (B) top-level dedup: folder + its child = ONE subtree ────────
        # f1 is expanded at boot, so f1-o0 is a visible descendant.
        tf.invoke(sel("select"), "f1")
        tf.invoke(sel("toggle"), "f1-o0")
        wait_until(lambda: selection() == ["f1", "f1-o0"], desc="folder + child selected")
        assert_eq(tf.invoke(sel("delete_selected"), None), 1, "ancestor+descendant = 1 subtree")
        # f1 (1) + its 12 children (12) leave the visible set.
        wait_until(lambda: rows() == BOOT_ROWS - 1 - OBJECTS_PER, desc="folder subtree removed")
        assert "f1" not in visible_ids() and "f1-o5" not in visible_ids(), "whole subtree gone"
        assert_eq(tf.invoke(sel("undo"), None), True, "undo")
        wait_until(lambda: rows() == BOOT_ROWS, desc="subtree restored")
        assert "f1-o0" in visible_ids(), "descendant came back with the folder"

        # ── (C) batch-rename with a {n} template; one undo step ──────────
        before = label_at("f3")
        tf.invoke(sel("select"), "f3")
        tf.invoke(sel("toggle"), "f4")
        wait_until(lambda: selection() == ["f3", "f4"], desc="two folders selected")
        assert_eq(tf.invoke(sel("rename_selected"), "Layer {n}"), 2, "two renamed")
        wait_until(lambda: label_at("f3") == "Layer 1", desc="f3 numbered 1")
        assert_eq(label_at("f4"), "Layer 2", "f4 numbered 2 (pre-order)")
        assert_eq(selection(), ["f3", "f4"], "rename keeps the selection")
        assert_eq(tf.invoke(sel("undo"), None), True, "undo rename")
        wait_until(lambda: label_at("f3") == before, desc="labels restored")

        # ── (D) literal template renames all the same ────────────────────
        tf.invoke(sel("select"), "f5")
        tf.invoke(sel("toggle"), "f6")
        wait_until(lambda: selection() == ["f5", "f6"], desc="two folders selected")
        assert_eq(tf.invoke(sel("rename_selected"), "Group"), 2, "two renamed to literal")
        wait_until(lambda: label_at("f5") == "Group", desc="f5 = Group")
        assert_eq(label_at("f6"), "Group", "f6 = Group")
        tf.invoke(sel("undo"), None)

        # ── (E) no-op safety: empty selection delete changes nothing ─────
        tf.invoke(sel("clear"), None)
        wait_until(lambda: selection() == [], desc="selection cleared")
        before_rows = rows()
        assert_eq(tf.invoke(sel("delete_selected"), None), 0, "empty selection deletes nothing")
        assert_eq(rows(), before_rows, "tree unchanged by an empty delete")

    # ── (F) live-pixel: the outliner rasterises ──────────────────────────
    img = read_png_rgba8(capture_screenshot())
    assert_eq((img.width, img.height), WIN, "screenshot matches viewport")
    ink = 0
    bg = png_pixel(img, 2, 2)[:3]
    for yy in range(0, img.height, 3):
        for xx in range(0, img.width, 3):
            r, g, b, _a = png_pixel(img, xx, yy)
            if (r - bg[0]) ** 2 + (g - bg[1]) ** 2 + (b - bg[2]) ** 2 > 900:
                ink += 1
    assert ink > 200, f"outliner rasterises many rows ({ink} ink samples)"


if __name__ == "__main__":
    sys.exit(run_demo("R905 outliner batch ops", body))
