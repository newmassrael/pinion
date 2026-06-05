#!/usr/bin/env python3
"""R791 §5.15 §5.38 — file-manager inline rename, driven over RPC.

The AI-first proof that the `Directory` write surface is complete with
**rename**, and that the inline-edit `TextField` (an extra-external modal-
member-style field that replaces the selected row's `file_row` in place) is a
real, introspectable text input. An agent selects an entry, starts a rename
(toolbar **Rename**, `F2`, or the direct `scene/invoke .../rename`), types a
new name into the field through `scene/key`, and commits with `Enter` — then
reads the renamed listing back as data (§2 #2 / #7). A native OS rename popup
is a window pinion does not own; this one is a pinion scene.

  (A) boot — the Rename button renders, disabled (nothing selected).
  (B) Rename gate — selecting a row enables Rename (its fill changes).
  (C) Rename by CLICK enters edit mode — the rename TextField replaces the
      selected row's file_row, pre-filled with the current name, focused;
      the plain file_row for that index is gone.
  (D) type + Enter commits — clearing the field and typing a new name via
      scene/key lands in the field; Enter renames the entry (new name in the
      listing, old gone), the selection follows, and edit mode exits.
  (E) F2 starts a rename with a selection.
  (F) Escape cancels — a typed name is discarded, nothing renamed.
  (G) rename by INVOKE (the direct AI path) — rename returns true + the
      selection follows; no-selection and taken-name renames return false.
  (H) directory rename carries the subtree — renaming a folder then
      navigating into the new name lists its preserved contents.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

EXAMPLE = "hello-file-manager"
VIEWPORT = (480, 540)
PAUSE = 0.12

DIR = "fb_dir"
NEWDIR = "fm_newdir"
RENAME = "fm_rename"
DELETE = "fm_delete"
NAME_TF = "fm_rename_tf"


def dpath(slot: str) -> str:
    return f"/{DIR}/external/{slot}"


def npath(slot: str) -> str:
    return f"/{NAME_TF}/external/{slot}"


def names(tf) -> list[str]:
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


def present(tf, tag) -> bool:
    return find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), tag) is not None


def rename_fill(tf):
    node = find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), RENAME)
    assert node is not None, "rename button present"
    fill = (node.get("style") or {}).get("fill") or {}
    return (fill.get("r"), fill.get("g"), fill.get("b"), fill.get("a"))


def focused(tf):
    return tf.request("focus/get").result.get("focused")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — Rename present + disabled ────────────────────
        assert present(tf, RENAME), "Rename button present"
        assert_eq(tf.query(dpath("selected")), None, "nothing selected at boot")
        disabled_fill = rename_fill(tf)
        assert not present(tf, NAME_TF), "no rename field until editing"

        # ── (B) Rename gate — selecting a row enables it ────────────
        tf.click(path=f"{DIR}#{row_index(tf, 'README.md')}")
        time.sleep(PAUSE)
        assert_eq(tf.query(dpath("selected")), "/proj/README.md", "row click selected the file")
        enabled_fill = rename_fill(tf)
        assert enabled_fill != disabled_fill, (
            f"Rename fill changes when a selection enables it: "
            f"disabled {disabled_fill} vs enabled {enabled_fill}"
        )

        # ── (C) Rename by CLICK enters edit mode ────────────────────
        sel_idx = row_index(tf, "README.md")
        tf.click(path=RENAME)
        time.sleep(PAUSE)
        assert present(tf, NAME_TF), "the rename field appears in the row"
        assert not present(tf, f"{DIR}#{sel_idx}"), "the selected file_row is replaced by the field"
        assert_eq(tf.query(npath("text")), "README.md", "field pre-filled with the current name")
        assert_eq(focused(tf), NAME_TF, "focus moved to the rename field")

        # ── (D) type a new name + Enter commits ─────────────────────
        tf.intervene(npath("text"), "")
        assert_eq(tf.query(npath("text")), "", "field cleared")
        tf.text("NOTES.md", path=NAME_TF)
        time.sleep(PAUSE)
        assert_eq(tf.query(npath("text")), "NOTES.md", "typed characters land in the field")
        tf.key(path=NAME_TF, name="Enter")
        time.sleep(PAUSE)
        assert "NOTES.md" in names(tf), f"Enter renamed the file; got {names(tf)}"
        assert "README.md" not in names(tf), "old name gone"
        assert_eq(tf.query(dpath("selected")), "/proj/NOTES.md", "selection follows the rename")
        assert not present(tf, NAME_TF), "edit mode exits on commit (field gone)"

        # ── (E) F2 starts a rename with a selection ─────────────────
        tf.click(path=f"{DIR}#{row_index(tf, 'Cargo.toml')}")
        time.sleep(PAUSE)
        tf.key(path=RENAME, name="F2")
        time.sleep(PAUSE)
        assert present(tf, NAME_TF), "F2 starts the inline rename"
        assert_eq(tf.query(npath("text")), "Cargo.toml", "F2 pre-fills the field")

        # ── (F) Escape cancels without renaming ─────────────────────
        tf.intervene(npath("text"), "")
        tf.text("WONT.toml", path=NAME_TF)
        time.sleep(PAUSE)
        tf.key(path=NAME_TF, name="Escape")
        time.sleep(PAUSE)
        assert not present(tf, NAME_TF), "Escape exits edit mode"
        assert "Cargo.toml" in names(tf), "Escape left the name unchanged"
        assert "WONT.toml" not in names(tf), "Escape renamed nothing"

        # ── (G) rename by INVOKE (the direct AI path) ───────────────
        tf.invoke(dpath("select"), "Cargo.toml")
        assert_eq(tf.invoke(dpath("rename"), "Cargo.lock"), True, "rename invoke returns success")
        assert "Cargo.lock" in names(tf), "renamed via invoke"
        assert_eq(tf.query(dpath("selected")), "/proj/Cargo.lock", "selection followed the invoke")
        # rename to a taken name → false; no-op leaves the listing.
        tf.invoke(dpath("select"), "Cargo.lock")
        assert_eq(tf.invoke(dpath("rename"), "NOTES.md"), False, "taken name rejected")
        assert "Cargo.lock" in names(tf), "rejected rename left the source"
        # rename with no selection → false.
        tf.invoke(dpath("open"), "/proj")  # clears selection
        assert_eq(tf.query(dpath("selected")), None, "selection cleared")
        assert_eq(tf.invoke(dpath("rename"), "x"), False, "rename with no selection returns false")

        # ── (H) directory rename carries its subtree ────────────────
        tf.invoke(dpath("select"), "src")
        assert_eq(tf.invoke(dpath("rename"), "source"), True, "directory rename succeeds")
        assert "source" in names(tf) and "src" not in names(tf), "folder renamed in the listing"
        assert_eq(tf.invoke(dpath("navigate"), "source"), "/proj/source", "navigate into the new name")
        inner = names(tf)
        assert "main.rs" in inner and "lib.rs" in inner, f"subtree preserved under new name; got {inner}"


if __name__ == "__main__":
    sys.exit(run_demo("R791 §5.15 §5.38 — file-manager inline rename", body))
