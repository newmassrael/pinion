#!/usr/bin/env python3
"""R1176 §5.38 §5.15 §5.39 — the property grid's embedded asset file picker.

A DCC inspector needs an **asset-reference** field: a property whose value is a
path to a project asset (a mesh / texture), picked from a file browser rather
than typed. R1176 adds one — the `Mesh` leaf (slot 1) is an *asset slot*
(`is_asset_slot`, the sibling of R964's ranged-Float refinement): a plain
`CellValue::Text` value (no new `CellKind`), but activating it opens an embedded
**modal file picker** instead of the inline text editor.

The picker is hand-assembled from the already-lifted own-rendered substrate (the
property grid is the Nth consumer of each, the 1st to embed a dialog in a host
widget): the R788 `ModalState` lifecycle, the R787 `DirectoryExternal` over an
`InMemoryDirectory`, the `view_dialog` chrome with `file_browser_pane` in its
body, and Cancel / Open action buttons. The chosen path commits through the
grid's `set_value` SSOT (so it reads back via `value.<i>` and is undoable like
any edit). The whole round-trip is AI-observable + driveable over the §5.12 RPC
plane (§2 #2): the picker opens via the grid's keyboard activation, the browser
navigates + selects via the `DirectoryExternal`, and OK / Cancel are clicks.

  (A) boot — the Mesh leaf holds the seeded asset path; the picker is shut.
  (B) open — activating the Mesh row opens the modal at /proj, nothing selected.
  (C) browse by INVOKE — navigate descends, up climbs, entries enumerate.
  (D) OK gate — with nothing selected, OK is a no-op and the modal stays open.
  (E) select — selecting a file records its full path; OK enables.
  (F) confirm — clicking OK writes the path into the Mesh slot and closes.
  (G) cancel — re-open, select, click Cancel: the value is left untouched.
  (H) Escape — re-open, Escape: a cancel, the value unchanged.

Run from the workspace root:
    cargo build -p hello-property-grid --release
    python3 tools/demos/r1176_property_grid_asset_picker.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

VIEWPORT = (460, 560)
GRID = "property_grid"
DIR = "asset_fb"
OK = "asset_ok"
CANCEL = "asset_cancel"
MESH = 1  # the Mesh leaf's flat-model value index


def dpath(slot: str) -> str:
    return f"/{DIR}/external/{slot}"


def modal_open(tf) -> bool:
    return tf.query("/asset_modal/external/open")


def mesh(tf) -> str:
    return tf.query(f"/external/value.{MESH}")


def open_picker(tf) -> None:
    """Open the embedded picker by activating the Mesh leaf over RPC —
    `invoke begin <mesh_slot>` (R1177: the picker is RPC-openable like the choice
    / colour popups, so an AI drives the whole open->browse->confirm round-trip;
    before R1177 this direct RPC begin no-op'd and the picker was GUI-only)."""
    tf.invoke("/external/begin", MESH)
    wait_until(lambda: modal_open(tf), timeout=4.0, desc="the picker opened over RPC")


def body() -> None:
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        assert_eq(mesh(tf), "/proj/meshes/hero.fbx", "Mesh boots at the seeded path")
        assert_eq(modal_open(tf), False, "the picker is shut at boot")

        # ── (B) open the picker over RPC (invoke begin — AI-driveable) ─
        open_picker(tf)
        assert_eq(modal_open(tf), True, "invoke begin on the Mesh slot opened the modal")
        assert_eq(tf.query(dpath("cwd")), "/proj", "the browser opens at /proj")
        assert_eq(tf.query(dpath("selected")), None, "nothing selected on open")
        assert_eq(tf.query(dpath("count")), 3, "/proj has 3 entries")

        # ── (C) browse by INVOKE (the DirectoryExternal) ─────────────
        assert_eq(tf.invoke(dpath("navigate"), "meshes"), "/proj/meshes", "navigate descends")
        assert_eq(tf.query(dpath("cwd")), "/proj/meshes", "cwd followed the navigate")
        assert_eq(tf.query(dpath("count")), 3, "meshes/ has 3 files")
        assert_eq(tf.query(dpath("is_dir.0")), False, "a mesh file is not a directory")
        assert_eq(tf.invoke(dpath("up"), None), "/proj", "up climbs to the parent")
        assert_eq(tf.query(dpath("cwd")), "/proj", "cwd followed the up")
        assert_eq(tf.invoke(dpath("navigate"), "textures"), "/proj/textures", "navigate textures")
        assert_eq(tf.query(dpath("count")), 2, "textures/ has 2 files")

        # ── (D) OK gate — nothing selected → OK is a no-op, modal stays ─
        assert_eq(tf.query(dpath("selected")), None, "still nothing selected")
        tf.click(path=OK)
        assert_eq(modal_open(tf), True, "OK with no selection does not close the modal")
        assert_eq(mesh(tf), "/proj/meshes/hero.fbx", "OK with no selection wrote nothing")

        # ── (E) select a file — its full path is recorded; OK enables ─
        assert_eq(
            tf.invoke(dpath("select"), "normal.png"),
            "/proj/textures/normal.png",
            "select records the full path",
        )
        assert_eq(tf.query(dpath("selected")), "/proj/textures/normal.png", "selected reads back")

        # ── (F) confirm — click OK writes the path + closes ──────────
        tf.click(path=OK)
        wait_until(lambda: not modal_open(tf), timeout=4.0, desc="OK closed the picker")
        assert_eq(mesh(tf), "/proj/textures/normal.png", "OK wrote the chosen path into Mesh")
        # The write went through the value SSOT — it is now modified vs default.
        assert_eq(tf.query(f"/external/modified.{MESH}"), True, "the Mesh slot reads as modified")

        # ── (G) cancel — re-open, select, click Cancel: untouched ────
        open_picker(tf)
        assert_eq(tf.invoke(dpath("navigate"), "meshes"), "/proj/meshes", "re-open + navigate")
        assert_eq(
            tf.invoke(dpath("select"), "enemy.fbx"),
            "/proj/meshes/enemy.fbx",
            "select a different asset",
        )
        tf.click(path=CANCEL)
        wait_until(lambda: not modal_open(tf), timeout=4.0, desc="Cancel closed the picker")
        assert_eq(mesh(tf), "/proj/textures/normal.png", "Cancel left the path untouched")

        # ── (H) Escape — re-open, Escape is a cancel ─────────────────
        open_picker(tf)
        assert_eq(modal_open(tf), True, "re-opened")
        tf.key(path=GRID, name="Escape")
        wait_until(lambda: not modal_open(tf), timeout=4.0, desc="Escape closed the picker")
        assert_eq(mesh(tf), "/proj/textures/normal.png", "Escape did not choose anew")


if __name__ == "__main__":
    sys.exit(run_demo("R1176 property-grid asset picker", body))
