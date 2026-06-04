#!/usr/bin/env python3
"""R787 §5.15 §5.16 — own-rendered file browser E2E.

Drives the `hello-file-browser` binding via JSON-RPC. Unlike a native (OS)
file dialog — a window pinion does not own, invisible to `scene/query` — this
browser is scene-graph-native: the current directory, its listing, and the
selection are introspectable data, and an AI agent navigates + selects through
`scene/query` + `scene/invoke` (the first consumer of the R787 `Directory`
read substrate, peer of R665 `Storage`).

The binding backs the `Directory` trait with a seeded `InMemoryDirectory`
(a synthetic sample project tree) so this flow is deterministic; the real
`FsDirectory` (std::fs::read_dir) is unit-tested in pinion-platform-storage.

  (A) boot — the model lists /proj's 5 entries (dirs-first, then alpha),
      each row's name + is_dir queryable; the wire `entries` suffixes dirs
      with '/'; nothing selected; the rows render as paint nodes.
  (B) navigate by RPC — `invoke navigate "src"` descends; the listing +
      breadcrumb track; `up` returns to the parent (and clears selection).
  (C) navigate by CLICK — `scene/click` on a directory row (`fb_dir#<i>`)
      descends through the same `send` wire a real pointer drives; clicking
      the `fb_dir#up` affordance climbs back.
  (D) select a file — clicking / invoking a file row records its full path
      in `selected`; the picked row paints Accent (Phase 2 pixel witness).
  (E) introspection — `intervene cwd` is an admin absolute jump; unknown
      paths + read-only slots are rejected.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame renders directory rows on a raised surface and file rows
    zebra-striped — assert a directory row and a file row paint in *distinct*
    inks, the live-pixel proof the dir/file affordance renders (which a
    structural query cannot replace, [[introspection-from-paint-not-screen]]).
    The selected-row Accent fill is covered by the binding's unit test (the
    boot frame has no selection — per-process state does not survive the
    separate PINION_SCREENSHOT render).
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-file-browser"
VIEWPORT = (420, 460)
DIR_TAG = "fb_dir"
PAUSE = 0.1


def dpath(slot: str) -> str:
    return f"/{DIR_TAG}/external/{slot}"


def cwd(tf) -> str:
    return tf.query(dpath("cwd"))


def count(tf) -> int:
    return tf.query(dpath("count"))


def entries(tf) -> str:
    return tf.query(dpath("entries"))


def selected(tf):
    return tf.query(dpath("selected"))


def near_rgb(a, b, tol=18) -> bool:
    return all(abs(int(a[i]) - int(b[i])) <= tol for i in range(3))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)

        # ── (A) boot model + rendered rows ──────────────────────────
        assert_eq(cwd(tf), "/proj", "opens at the project root")
        assert_eq(count(tf), 5, "five entries in /proj")
        # Dirs first (assets, src), then files alpha (.gitignore, Cargo.toml, README.md).
        assert_eq(tf.query(dpath("name.0")), "assets", "row 0 = assets (dir)")
        assert_eq(tf.query(dpath("is_dir.0")), True, "assets is a directory")
        assert_eq(tf.query(dpath("name.1")), "src", "row 1 = src (dir)")
        assert_eq(tf.query(dpath("name.2")), ".gitignore", "files sort after dirs")
        assert_eq(tf.query(dpath("is_dir.2")), False, ".gitignore is a file")
        assert_eq(
            entries(tf),
            "assets/\nsrc/\n.gitignore\nCargo.toml\nREADME.md",
            "wire listing suffixes dirs with '/'",
        )
        assert_eq(selected(tf), None, "nothing selected at boot")
        # Rows render as paint nodes.
        for i in range(5):
            assert find_by_tag(snap, f"{DIR_TAG}#{i}") is not None, f"row {i} painted"
        assert find_by_tag(snap, f"{DIR_TAG}#up") is not None, "parent affordance painted"

        # ── (B) navigate by RPC invoke ──────────────────────────────
        assert_eq(tf.invoke(dpath("navigate"), "src"), "/proj/src", "navigate returns the new cwd")
        assert_eq(cwd(tf), "/proj/src", "descended into src")
        assert_eq(count(tf), 3, "src has 3 entries")
        assert_eq(tf.query(dpath("name.0")), "ui", "src's dir sorts first")
        assert_eq(tf.invoke(dpath("navigate"), "README.md"), "/proj/src", "navigate to a file is a no-op")
        assert_eq(tf.invoke(dpath("up"), None), "/proj", "up returns to the parent")
        assert_eq(count(tf), 5, "parent listing restored")

        # ── (C) navigate + climb by CLICK (the pointer send wire) ───
        def row_index(tf, name: str) -> int:
            n = count(tf)
            for i in range(n):
                if tf.query(dpath(f"name.{i}")) == name:
                    return i
            raise AssertionError(f"{name} not in listing")

        tf.click(path=f"{DIR_TAG}#{row_index(tf, 'assets')}")
        time.sleep(PAUSE)
        assert_eq(cwd(tf), "/proj/assets", "clicking a directory row descends")
        assert_eq(count(tf), 2, "assets has 2 files")
        tf.click(path=f"{DIR_TAG}#up")
        time.sleep(PAUSE)
        assert_eq(cwd(tf), "/proj", "clicking the parent affordance climbs")

        # ── (D) select a file (by click + by invoke) ────────────────
        tf.click(path=f"{DIR_TAG}#{row_index(tf, 'Cargo.toml')}")
        time.sleep(PAUSE)
        assert_eq(selected(tf), "/proj/Cargo.toml", "clicking a file row selects it")
        # Re-select via the explicit invoke (the AI-first path).
        assert_eq(
            tf.invoke(dpath("select"), "README.md"),
            "/proj/README.md",
            "select returns the picked path",
        )
        assert_eq(selected(tf), "/proj/README.md", "selection updated")
        # Navigating clears the selection (new directory context).
        tf.invoke(dpath("navigate"), "src")
        assert_eq(selected(tf), None, "navigation clears the selection")
        tf.invoke(dpath("up"), None)

        # ── (E) introspection: admin jump + rejections ──────────────
        tf.intervene(dpath("cwd"), "/proj/src/ui")
        assert_eq(cwd(tf), "/proj/src/ui", "intervene cwd jumps absolutely")
        assert_eq(count(tf), 2, "ui has 2 files")
        tf.intervene(dpath("cwd"), "/proj")  # reset
        raised = False
        try:
            tf.intervene(dpath("count"), 0)
        except RpcError:
            raised = True
        assert raised, "count is read-only (intervene rejected)"
        raised = False
        try:
            tf.query(dpath("no_such_slot"))
        except RpcError:
            raised = True
        assert raised, "unknown introspect path rejected"

    # ── Phase 2 — live pixels: dir vs file rows render distinctly ───
    dir_and_file_rows_render_distinctly()


def dir_and_file_rows_render_distinctly() -> None:
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    if not binary.exists():
        print("  PHASE 2 SKIP: release binary not built")
        return
    try:
        from PIL import Image  # noqa: F401
    except ImportError:
        print("  PHASE 2 SKIP: Pillow unavailable")
        return

    # Read the boot-frame row rects (a fresh subprocess, same boot state the
    # PINION_SCREENSHOT render captures: /proj, nothing selected).
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        rects = abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))
        # Row 0 = assets (directory), row 2 = .gitignore (file).
        assert_eq(tf.query(dpath("is_dir.0")), True, "row 0 is a directory")
        assert_eq(tf.query(dpath("is_dir.2")), False, "row 2 is a file")
    dir_tag, file_tag = f"{DIR_TAG}#0", f"{DIR_TAG}#2"
    assert dir_tag in rects and file_tag in rects, "both rows have paint rects"

    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"

    def fill_at(tag: str):
        x, y, _w, h = rects[tag]
        # Sample left of the glyphs (a clean fill band).
        return sample_png_points(img, [(x + 5, y + h // 2)])[0]

    dir_px = fill_at(dir_tag)
    file_px = fill_at(file_tag)
    assert not near_rgb(dir_px, file_px, tol=6), (
        f"directory row {dir_px[:3]} and file row {file_px[:3]} must render in "
        "distinct inks (the dir-vs-file affordance)"
    )
    print(f"  PHASE 2 OK: dir row {dir_px[:3]} != file row {file_px[:3]} (distinct affordance)")


def capture_screenshot() -> Path:
    """Render the boot frame to RGBA8 PNG via `PINION_SCREENSHOT`, the same
    `to_vello_cached` rasterizer the live window uses (producer parity)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r787-")) / "browser.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        [str(binary)], cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0 or not out.exists():
        raise AssertionError(
            f"PINION_SCREENSHOT capture failed (rc={res.returncode}): {res.stderr[-400:]!r}"
        )
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R787 §5.15 — own-rendered file browser", body))
