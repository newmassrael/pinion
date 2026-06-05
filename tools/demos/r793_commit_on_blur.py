#!/usr/bin/env python3
"""R793 §5.38 §5.39 — commit-on-blur for inline editors.

Drives the `hello-file-manager` binding via JSON-RPC. An inline editor's
`TextField` (opted in via `TextFieldExternal::with_blur_intent`) emits a
`"blur"` §5.20 intent on every focus loss — the W3C DOM `focusout` mirror —
and the binding's reducer **commits the edit** on it (click-away saves, the
Files/Explorer / TodoMVC convention). Without stealing focus back: the click
that caused the blur already moved focus where the user wants it.

The decisive witness is scene-as-data + RPC focus (§2 #2 / #7): start a
rename, type, move focus to another widget (`focus/set`), and observe the
rename committed in the listing — with focus left on the new target, not
yanked back to the editor's trigger.

  (A) boot + enter rename — select a file, Rename, the field appears focused.
  (B) commit-on-blur — type a new name, move focus to a toolbar button; the
      rename commits, the selection follows, edit mode exits, and focus stays
      on the button (not stolen back to Rename).
  (C) Enter still commits + restores focus (the keyboard path is unchanged).
  (D) Escape still cancels (blur-after-cancel is inert — renaming already off).
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


def focused(tf):
    return tf.request("focus/get").result.get("focused")


def start_rename(tf, name: str) -> None:
    """Select `name` and enter inline rename (field focused, pre-filled)."""
    tf.click(path=f"{DIR}#{row_index(tf, name)}")
    time.sleep(PAUSE)
    tf.click(path=RENAME)
    time.sleep(PAUSE)
    assert present(tf, NAME_TF), "the rename field appears"
    assert_eq(focused(tf), NAME_TF, "focus moved to the rename field")


def type_name(tf, new: str) -> None:
    tf.intervene(npath("text"), "")
    tf.text(new, path=NAME_TF)
    time.sleep(PAUSE)
    assert_eq(tf.query(npath("text")), new, "typed characters land in the field")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot + enter rename ─────────────────────────────────
        assert present(tf, RENAME), "Rename button present"
        assert_eq(tf.query(dpath("selected")), None, "nothing selected at boot")
        assert_eq(count(tf), 4, "four entries in /proj at boot")
        assert not present(tf, NAME_TF), "no rename field until editing"
        start_rename(tf, "README.md")
        assert_eq(tf.query(dpath("selected")), "/proj/README.md", "the renamed row is selected")
        assert_eq(tf.query(npath("text")), "README.md", "field pre-filled")

        # ── (B) commit-on-blur: focus another widget commits the edit ─
        cnt = count(tf)
        type_name(tf, "NOTES.md")
        assert present(tf, NAME_TF), "the field is live while editing"
        # Move focus to a toolbar button — the rename field blurs.
        tf.request("focus/set", {"tag": NEWDIR})
        time.sleep(PAUSE)
        assert "NOTES.md" in names(tf), f"blur committed the rename; got {names(tf)}"
        assert "README.md" not in names(tf), "old name gone after blur-commit"
        assert_eq(count(tf), cnt, "a rename keeps the entry count (not an add/delete)")
        assert_eq(tf.query(dpath("selected")), "/proj/NOTES.md", "selection follows the rename")
        assert not present(tf, NAME_TF), "edit mode exits on blur (field gone)"
        assert_eq(focused(tf), NEWDIR, "focus stays on the button — not stolen back to Rename")

        # ── (B2) a second blur (no edit in progress) is inert ───────
        before = names(tf)
        tf.request("focus/set", {"tag": RENAME})
        time.sleep(PAUSE)
        tf.request("focus/set", {"tag": NEWDIR})
        time.sleep(PAUSE)
        assert_eq(names(tf), before, "blur with no edit in progress changes nothing")
        assert_eq(focused(tf), NEWDIR, "the inert blur leaves focus put")

        # ── (C) Enter still commits + restores focus (unchanged) ────
        start_rename(tf, "NOTES.md")
        type_name(tf, "DONE.md")
        tf.key(path=NAME_TF, name="Enter")
        time.sleep(PAUSE)
        assert "DONE.md" in names(tf), "Enter still commits the rename"
        assert "NOTES.md" not in names(tf), "old name gone after Enter"
        assert_eq(count(tf), cnt, "Enter-commit keeps the entry count")
        assert not present(tf, NAME_TF), "Enter exits edit mode"
        assert_eq(focused(tf), RENAME, "Enter restores focus to the Rename button")

        # ── (D) Escape still cancels (no blur-commit afterwards) ────
        start_rename(tf, "DONE.md")
        type_name(tf, "WONT.md")
        assert_eq(tf.query(npath("text")), "WONT.md", "the discard-me text is in the field")
        tf.key(path=NAME_TF, name="Escape")
        time.sleep(PAUSE)
        assert not present(tf, NAME_TF), "Escape exits edit mode"
        assert "DONE.md" in names(tf), "Escape left the name unchanged"
        assert "WONT.md" not in names(tf), "Escape renamed nothing"
        assert_eq(count(tf), cnt, "Escape changes no entry count")
        # Escape restored focus to Rename; a later blur is inert (not editing).
        assert_eq(focused(tf), RENAME, "Escape restores focus to the Rename button")
        tf.request("focus/set", {"tag": NEWDIR})
        time.sleep(PAUSE)
        assert "DONE.md" in names(tf), "post-cancel blur does not resurrect a commit"
        assert "WONT.md" not in names(tf), "post-cancel blur commits nothing"


if __name__ == "__main__":
    sys.exit(run_demo("R793 §5.38 §5.39 — commit-on-blur for inline editors", body))
