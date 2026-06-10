#!/usr/bin/env python3
"""R789 §3 §5.15 — own-rendered file manager (Directory write surface), over RPC.

The AI-first proof that the filesystem write hatch is complete and
introspectable: an agent creates folders + files and deletes entries, then
reads the resulting listing back as data — through `scene/click` (the toolbar
UI path) AND `scene/invoke /fb_dir/external/{mkdir,touch,delete}` (the direct
AI path), with `scene/query .../entries` confirming each mutation (§2 #2 / #7).
A native OS file manager could not be driven or verified this way at all
([[native-menu-macos-windows-only-verify]]).

The binding backs the `Directory` trait with a seeded `InMemoryDirectory`
(deterministic), so this flow is reproducible; the real-fs `FsDirectory` write
side (`std::fs::create_dir` / `File::create` / `remove_dir_all`) is unit-tested
in pinion-platform-storage.

  (A) boot — /proj lists 4 entries; the toolbar (New Folder / New File /
      Delete) renders; Delete paints its disabled fill (nothing selected).
  (B) New Folder by CLICK — clicking the toolbar button creates "New Folder";
      a second click auto-names "New Folder 2" (no duplicate failure).
  (C) New File by CLICK — creates "untitled.txt".
  (D) Delete by CLICK — selecting a row enables Delete (fill changes); the
      click removes the entry and clears the selection.
  (E) write by INVOKE (the direct AI path) — mkdir "docs" + touch "LICENSE"
      return true and appear; a duplicate mkdir returns false (not an error);
      select + delete removes by RPC.
  (F) Delete gate — with nothing selected, the delete invoke returns false.
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
    wait_until,
)

EXAMPLE = "hello-file-manager"
VIEWPORT = (460, 520)

DIR = "fb_dir"
NEWDIR = "fm_newdir"
NEWFILE = "fm_newfile"
DELETE = "fm_delete"


def dpath(slot: str) -> str:
    return f"/{DIR}/external/{slot}"


def names(tf) -> list[str]:
    """Current listing leaf names (dirs lose the wire '/' suffix)."""
    wire = tf.query(dpath("entries"))
    if not wire:
        return []
    return [n[:-1] if n.endswith("/") else n for n in wire.split("\n")]


def count(tf) -> int:
    return tf.query(dpath("count"))


def row_index(tf, name: str) -> int:
    for i in range(count(tf)):
        if tf.query(dpath(f"name.{i}")) == name:
            return i
    raise AssertionError(f"{name} not in listing {names(tf)}")


def delete_fill(tf):
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, DELETE)
    assert node is not None, "delete button present"
    fill = (node.get("style") or {}).get("fill") or {}
    return (fill.get("r"), fill.get("g"), fill.get("b"), fill.get("a"))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot ────────────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, NEWDIR) is not None, "New Folder button present"
        assert find_by_tag(snap, NEWFILE) is not None, "New File button present"
        assert find_by_tag(snap, DELETE) is not None, "Delete button present"
        assert_eq(count(tf), 4, "/proj boots with four entries")
        assert "assets" in names(tf) and "Cargo.toml" in names(tf), "seeded entries listed"
        assert_eq(tf.query(dpath("selected")), None, "nothing selected at boot")
        disabled_fill = delete_fill(tf)

        # ── (B) New Folder by CLICK (auto-named) ────────────────────
        tf.click(path=NEWDIR)
        wait_until(lambda: "New Folder" in names(tf), desc="click created New Folder")
        assert_eq(count(tf), 5, "listing grew by one")
        tf.click(path=NEWDIR)
        wait_until(lambda: "New Folder 2" in names(tf),
                   desc="second click auto-names New Folder 2")
        assert_eq(count(tf), 6, "listing grew again")

        # ── (C) New File by CLICK ───────────────────────────────────
        tf.click(path=NEWFILE)
        wait_until(lambda: "untitled.txt" in names(tf), desc="click created untitled.txt")

        # ── (D) Delete by CLICK (gated on a selection) ──────────────
        tf.click(path=f"{DIR}#{row_index(tf, 'README.md')}")
        wait_query(tf, dpath("selected"), "/proj/README.md",
                   desc="row click selected the file")
        enabled_fill = delete_fill(tf)
        assert enabled_fill != disabled_fill, (
            f"Delete fill changes when a selection enables it: "
            f"disabled {disabled_fill} vs enabled {enabled_fill}"
        )
        before = count(tf)
        tf.click(path=DELETE)
        wait_until(lambda: "README.md" not in names(tf),
                   desc="Delete removed the selected file")
        assert_eq(count(tf), before - 1, "listing shrank by one")
        assert_eq(tf.query(dpath("selected")), None, "selection cleared after delete")

        # ── (E) write by INVOKE (the direct AI path) ────────────────
        assert_eq(tf.invoke(dpath("mkdir"), "docs"), True, "mkdir returns success")
        assert "docs" in names(tf), "mkdir created docs"
        assert_eq(tf.invoke(dpath("touch"), "LICENSE"), True, "touch returns success")
        assert "LICENSE" in names(tf), "touch created LICENSE"
        assert_eq(tf.invoke(dpath("mkdir"), "docs"), False, "duplicate mkdir returns false (no error)")
        # Navigate into the new dir: it lists empty.
        assert_eq(tf.invoke(dpath("navigate"), "docs"), "/proj/docs", "navigate into the new dir")
        assert_eq(count(tf), 0, "the freshly created dir is empty")
        tf.invoke(dpath("up"), None)

        # select + delete by invoke
        tf.invoke(dpath("select"), "LICENSE")
        assert_eq(tf.query(dpath("selected")), "/proj/LICENSE", "selected LICENSE by invoke")
        assert_eq(tf.invoke(dpath("delete"), None), True, "delete (selected) returns success")
        assert "LICENSE" not in names(tf), "LICENSE removed by invoke"
        assert_eq(tf.query(dpath("selected")), None, "selection cleared")

        # ── (F) Delete gate — no selection → invoke is a false no-op ─
        assert_eq(tf.query(dpath("selected")), None, "nothing selected")
        assert_eq(tf.invoke(dpath("delete"), None), False, "delete with no selection returns false")


if __name__ == "__main__":
    sys.exit(run_demo("R789 §3 §5.15 — own-rendered file manager (Directory write)", body))
