#!/usr/bin/env python3
"""R770 §5.15 — OS file drag-drop: hover / cancel / drop over RPC peers.

winit normalises the platform file-DnD (X11 XdndDrop / Wayland
data-device / macOS NSDraggingDestination / Windows IDropTarget) into
three window-scoped events — HoveredFile / HoveredFileCancelled /
DroppedFile — which the shell routes to the `WidgetView` file hooks. The
`scene/hover_file`, `scene/hover_file_cancel`, and `scene/drop_file` RPC
methods are their AI-first peers (§2 invariant #2). hello-filedrop is a
drop-zone that highlights on hover and lists dropped paths.

This demo proves, all without OCR (the drop-zone state is scene-as-data):

1. **Idle** — the zone shows the "Drag a file here" hint, 0 dropped.
2. **Hover** (`scene/hover_file`) — the zone hint flips to "Release to
   drop" (the binding's `on_file_hover` lit the affordance).
3. **Cancel** (`scene/hover_file_cancel`) — back to the idle hint.
4. **Drop** (`scene/hover_file` → `scene/drop_file`) — the dropped path
   appears as a `Text` node, the hover affordance clears, the count
   increments.
5. **Multiple** — a second drop appends; both paths are listed.
6. **Live-pixel** — a boot-frame `PINION_SCREENSHOT` renders the drop
   zone distinct from the window background (the zone fill + border).

Run from the workspace root:
    cargo build -p hello-filedrop --release
    python3 tools/demos/r770_filedrop.py
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
    abs_rects_of,
    assert_eq,
    find_by_tag,
    png_pixel,
    read_png_rgba8,
    run_demo,
    wait_until,
)

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
ZONE_TAG = "drop_zone"
VIEWPORT = (480, 320)


def _walk(node, out):
    out.append(node)
    for c in node.get("children", []) or []:
        _walk(c, out)
    if isinstance(node.get("content"), dict):
        _walk(node["content"], out)
    return out


def zone_texts(ta: RpcSubprocess) -> list[str]:
    """The `Text` contents inside the drop-zone container (from:paint)."""
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    zone = find_by_tag(snap, ZONE_TAG)
    assert zone is not None, "drop zone present in the paint scene"
    return [
        n["content"]
        for n in _walk(zone, [])
        if n.get("type") == "Text" and isinstance(n.get("content"), str)
    ]


def path_count(ta: RpcSubprocess) -> int:
    """How many absolute-path lines are listed in the zone."""
    return sum(1 for t in zone_texts(ta) if t.startswith("/"))


def status_line(ta: RpcSubprocess) -> str:
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    for n in _walk(snap, []):
        if n.get("type") == "Text" and isinstance(n.get("content"), str) and "dropped" in n["content"]:
            return n["content"]
    return ""


def hover_file(ta: RpcSubprocess, path: str) -> None:
    ta.request("scene/hover_file", {"path": path})


def hover_cancel(ta: RpcSubprocess) -> None:
    ta.request("scene/hover_file_cancel")


def drop_file(ta: RpcSubprocess, path: str) -> None:
    ta.request("scene/drop_file", {"path": path})


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r770-")) / "filedrop.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-filedrop"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-filedrop", "--quiet", "--release",
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
    zone_rect = None
    with RpcSubprocess("hello-filedrop", request_timeout=12.0) as ta:
        # ── Phase 1 — idle ───────────────────────────────────────────
        assert "Drag a file here" in zone_texts(ta), "idle hint shown"
        assert_eq(status_line(ta), "0 file(s) dropped", "no drops at boot")
        assert path_count(ta) == 0, "no path lines at boot"
        zone_rect = abs_rects_of(ta.snapshot(source="paint", viewport=VIEWPORT))[ZONE_TAG]
        assert zone_rect[2] > 0 and zone_rect[3] > 0, "drop zone has a non-empty rect"

        # ── Phase 2 — hover lights the affordance ─────────────────────
        hover_file(ta, "/tmp/photo.png")
        wait_until(
            lambda: "Release to drop" in zone_texts(ta),
            desc="hover flips the zone hint to 'Release to drop'",
        )
        assert "Drag a file here" not in zone_texts(ta), "idle hint replaced while hovering"
        assert "Release to drop" in zone_texts(ta), "hover affordance text shown exactly"

        # ── Phase 3 — cancel clears it ────────────────────────────────
        hover_cancel(ta)
        wait_until(
            lambda: "Drag a file here" in zone_texts(ta),
            desc="cancel restores the idle hint",
        )
        assert "Release to drop" not in zone_texts(ta), "hover affordance cleared on cancel"
        assert_eq(status_line(ta), "0 file(s) dropped", "cancel drops nothing")

        # ── Phase 4 — drop appends the path + clears hover ────────────
        hover_file(ta, "/home/me/report.pdf")
        wait_until(lambda: "Release to drop" in zone_texts(ta), desc="hover before drop")
        drop_file(ta, "/home/me/report.pdf")
        wait_until(
            lambda: "/home/me/report.pdf" in zone_texts(ta),
            desc="dropped path appears in the zone",
        )
        assert "Release to drop" not in zone_texts(ta), "drop cleared the hover affordance"
        assert_eq(status_line(ta), "1 file(s) dropped", "count incremented to 1")

        # Hover is idempotent — a repeated HoveredFile keeps the
        # affordance lit (winit re-fires HoveredFile as the cursor moves).
        hover_file(ta, "/home/me/report.pdf")
        hover_file(ta, "/home/me/report.pdf")
        wait_until(lambda: "Release to drop" in zone_texts(ta), desc="repeated hover stays lit")
        hover_cancel(ta)
        wait_until(lambda: "Release to drop" not in zone_texts(ta), desc="cancel after repeat hover")

        # ── Phase 5 — a second drop appends (order preserved) ─────────
        drop_file(ta, "/home/me/notes.txt")
        wait_until(
            lambda: "/home/me/notes.txt" in zone_texts(ta),
            desc="second dropped path appears",
        )
        texts = zone_texts(ta)
        assert "/home/me/report.pdf" in texts, "first path still listed"
        assert "/home/me/notes.txt" in texts, "second path listed"
        assert_eq(status_line(ta), "2 file(s) dropped", "count incremented to 2")
        assert path_count(ta) == 2, "exactly two path lines listed"
        # Drops list in arrival order (the list is append-only).
        i_first = texts.index("/home/me/report.pdf")
        i_second = texts.index("/home/me/notes.txt")
        assert i_first < i_second, "dropped paths list in arrival order"

        # ── Phase 6 — hover over a populated zone hides the list ───────
        hover_file(ta, "/tmp/x.bin")
        wait_until(lambda: "Release to drop" in zone_texts(ta), desc="hover over populated zone")
        assert "/home/me/report.pdf" not in zone_texts(ta), "the list yields to the hover affordance"
        hover_cancel(ta)
        wait_until(
            lambda: "/home/me/report.pdf" in zone_texts(ta),
            desc="cancel restores the dropped-path list",
        )
        assert "/home/me/notes.txt" in zone_texts(ta), "both paths return after cancel"
        assert_eq(status_line(ta), "2 file(s) dropped", "cancel-over-populated drops nothing")

        # ── Phase 7 — a path with spaces + non-ASCII rides through ─────
        unicode_path = "/tmp/내 파일 résumé.txt"
        drop_file(ta, unicode_path)
        wait_until(lambda: unicode_path in zone_texts(ta), desc="unicode/space path dropped")
        assert_eq(status_line(ta), "3 file(s) dropped", "count is 3 after the unicode drop")
        assert path_count(ta) == 3, "exactly three path lines after the unicode drop"

        # ── Phase 8 — bulk drop loop accumulates every path ───────────
        bulk = [f"/data/file_{i}.dat" for i in range(4)]
        for p in bulk:
            drop_file(ta, p)
        wait_until(
            lambda: all(p in zone_texts(ta) for p in bulk),
            desc="all bulk-dropped paths appear",
        )
        final_texts = zone_texts(ta)
        for p in bulk:
            assert p in final_texts, f"bulk path {p} listed"
        assert_eq(status_line(ta), "7 file(s) dropped", "3 + 4 bulk = 7 total")
        assert path_count(ta) == 7, "exactly seven path lines after the bulk drop"
        # The earliest drops survive the bulk append (append-only list).
        assert "/home/me/report.pdf" in final_texts, "earliest drop still present after bulk"
        assert "/home/me/notes.txt" in final_texts, "second drop still present after bulk"

    # ── Phase 9 — live-pixel: the zone renders distinct from the bg ───
    assert zone_rect is not None
    img = read_png_rgba8(capture_screenshot())
    assert_eq((img.width, img.height), VIEWPORT, "screenshot matches viewport")
    # The window background (a corner) vs the drop-zone centre: the zone
    # has its own fill + border, so it must be visually distinct — a
    # theme-independent "the zone is rendered" guard.
    bg = png_pixel(img, 4, 4)[:3]
    zx = zone_rect[0] + zone_rect[2] // 2
    zy = zone_rect[1] + zone_rect[3] // 2
    # Sample a band across the zone; require some pixel that differs from
    # the bare window background (fill tier shift or the outline border).
    distinct = False
    for yy in range(max(0, zone_rect[1] + 2), min(img.height, zone_rect[1] + zone_rect[3] - 2)):
        r, g, b, _a = png_pixel(img, zx, yy)
        if (r - bg[0]) ** 2 + (g - bg[1]) ** 2 + (b - bg[2]) ** 2 > 64:
            distinct = True
            break
    assert distinct, "the drop zone renders distinct from the window background"
    # The zone centre itself is inside the rendered surface (sanity).
    assert 0 <= zx < img.width and 0 <= zy < img.height, "zone centre within the frame"


if __name__ == "__main__":
    sys.exit(run_demo("R770 OS file drag-drop", body))
