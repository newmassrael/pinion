#!/usr/bin/env python3
"""R761 §3 §5.15 FileDialog — Open / Save / Pick-folder via the scripted mock.

`hello-file-dialog` drives the deterministic `ScriptedFileDialog`
(pinion-core) through the §5.22 `Resource` async-reactive carrier — the
headless, RPC-drivable, AI-first verification channel for the native
file-dialog capability (the real `RfdFileDialog` in
`pinion-platform-file-dialog` cannot run headless; that trait-split *is*
the discipline). Three buttons launch dialogs; the chosen path (or
cancellation) renders as a tagged `role=status` line, observed as DATA
through `scene/snapshot` (§2 #7 scene-as-data) — no pixels needed.

The binding pre-seeds a FIFO choice script (the sequence of choices the
"user" makes, consumed across all dialog calls):

  1. /projects/alpha.pinion.xml   (Open → selected)
  2. cancel                       (Open → cancelled)
  3. /exports/diagram.svg         (Save → selected)
  4. /home/user/assets            (Pick → selected)
  5. /keyboard/activated.txt      (keyboard Enter → selected)

The demo walks that script via `scene/click` + `scene/key`, asserting
the rendered status line + button structure at each step. After the
queue is exhausted, further dialogs resolve to cancellation (empty
queue == user cancels), which the tail of the demo also verifies.

Run from the workspace root:
    cargo build -p hello-file-dialog --release
    python3 tools/demos/r761_file_dialog.py
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
    wait_until,
)

VIEWPORT = (420, 220)

OPEN_TAG = "open_file"
SAVE_TAG = "save_file"
PICK_TAG = "pick_folder"
STATUS_TAG = "dialog_status"


def find_text(node, content: str):
    """Depth-first search for a Text node whose content == content."""
    if not isinstance(node, dict):
        return None
    if node.get("type") == "Text" and node.get("content") == content:
        return node
    for child in node.get("children", []) or []:
        hit = find_text(child, content)
        if hit is not None:
            return hit
    return None


def status_of(snap):
    """The current rendered status line (the STATUS_TAG container's text)."""
    node = find_by_tag(snap, STATUS_TAG)
    if node is None:
        return None
    for child in node.get("children", []) or []:
        if child.get("type") == "Text":
            return child.get("content")
    return None


def assert_three_buttons(snap, where: str) -> None:
    """The three dialog-launching buttons stay present across every step."""
    for tag in (OPEN_TAG, SAVE_TAG, PICK_TAG):
        assert find_by_tag(snap, tag) is not None, f"{tag} present ({where})"


def wait_status(d, expected: str, where: str):
    """Poll the paint scene until the status line reaches `expected`.

    The dialog result lands in the `Resource` on the shell frame after
    the click/key applies, and `scene/snapshot from=paint` reads the
    last rendered frame — so poll the observed status instead of a fixed
    sleep (sweep-load robust, per [[introspection-from-paint-not-screen]]).
    Returns the snapshot whose status matched.
    """
    snap_box: dict = {}

    def matched() -> bool:
        snap_box["snap"] = d.snapshot(source="paint", viewport=VIEWPORT)
        return status_of(snap_box["snap"]) == expected

    wait_until(matched, desc=f"status == {expected!r} ({where})")
    return snap_box["snap"]


def body() -> None:
    with RpcSubprocess("hello-file-dialog") as d:
        # ── boot: nothing chosen, all three buttons + labels present ──
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(status_of(snap), "No file chosen yet", "boot status")
        assert find_text(snap, "File dialog (scripted)") is not None, "title present"
        assert find_text(snap, "Open") is not None, "Open label present"
        assert find_text(snap, "Save") is not None, "Save label present"
        assert find_text(snap, "Pick folder") is not None, "Pick-folder label present"
        assert_three_buttons(snap, "boot")

        # ── Open → scripted selection #1 ─────────────────────────────
        d.click(path=OPEN_TAG)
        snap = wait_status(d, "Opened: /projects/alpha.pinion.xml", "open #1 selected")
        assert status_of(snap) != "No file chosen yet", "status changed off boot"
        assert_three_buttons(snap, "after open #1")

        # ── Open again → scripted cancel #2 ──────────────────────────
        d.click(path=OPEN_TAG)
        snap = wait_status(d, "Open cancelled", "open #2 cancelled (FIFO)")
        assert_three_buttons(snap, "after open #2")

        # ── Save → scripted selection #3 (carries suggested name) ────
        d.click(path=SAVE_TAG)
        snap = wait_status(d, "Saving to: /exports/diagram.svg", "save #3 selected")
        assert_three_buttons(snap, "after save")

        # ── Pick folder → scripted selection #4 ──────────────────────
        d.click(path=PICK_TAG)
        snap = wait_status(d, "Folder: /home/user/assets", "pick #4 selected")
        assert_three_buttons(snap, "after pick")

        # ── keyboard activation: focus Open, press Enter → #5 ────────
        # Proves the same dialog flow fires from the keyboard activation
        # arc (apply_aria_activate), not just pointer clicks.
        d.request("focus/set", {"tag": OPEN_TAG})
        d.key(path=OPEN_TAG, name="Enter")
        snap = wait_status(d, "Opened: /keyboard/activated.txt", "keyboard Enter selected #5")
        assert status_of(snap) != "Folder: /home/user/assets", "status changed via keyboard"
        assert_three_buttons(snap, "after keyboard")

        # ── queue exhausted: further dialogs resolve to cancellation ─
        d.click(path=SAVE_TAG)
        snap = wait_status(d, "Save cancelled", "exhausted queue → save cancelled")

        d.click(path=PICK_TAG)
        snap = wait_status(d, "Pick folder cancelled", "exhausted queue → pick cancelled")

        d.click(path=OPEN_TAG)
        snap = wait_status(d, "Open cancelled", "exhausted queue → open cancelled")
        assert_three_buttons(snap, "after exhaustion")

        # ── structure intact + labels persist through every transition ─
        assert find_text(snap, "Open") is not None, "Open label persists"
        assert find_text(snap, "Save") is not None, "Save label persists"
        assert find_text(snap, "Pick folder") is not None, "Pick-folder label persists"
        assert find_text(snap, "File dialog (scripted)") is not None, "title persists"
        assert find_by_tag(snap, STATUS_TAG) is not None, "status region persists"


if __name__ == "__main__":
    sys.exit(run_demo("R761 FileDialog scripted Open/Save/Pick", body))
