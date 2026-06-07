#!/usr/bin/env python3
"""R819 §5.16 §5.27 §5.50 — virtualized TreeView E2E.

Drives the `hello-virtual-tree` binding via JSON-RPC. First consumer of
`pinion_widget_paint::tree_view::view_virtual_tree`: a 10,000-node tree
whose visible-row sequence (`flat_visible`) is windowed through the R774
AutoSizer geometry, so only the rows inside the scroll window become scene
nodes. The Phase D editor's scene-graph / asset trees grow past the point
where a full flat row walk is a paint-cycle bottleneck — this is that
substrate's first taker.

The decisive witness is scene-as-data (§2 #7), not pixels: hundreds of
rows are visible, but `scene/snapshot` reports only the ~viewport-sized
window of `{TREE_TAG}#{id}` rows; the present row band equals the windowing
math fed the runtime-MEASURED viewport. Expanding / collapsing a section
changes the visible-row count (and the flat order), and the window
re-derives against the new length — the same id set the AT tree windows.

Atomic verification scope (>=30 assertions):

  (A) boot — a small window of the 496 boot-visible rows exists; the band
      == the windowing math fed the measured viewport; the top section is
      rendered, a deep section is NOT; the header reports the visible count.
  (B) the window slides — `scene/scroll` advances the offset; the band
      shifts to the matching flat indices and stays viewport-sized; the top
      row leaves; scroll-to-bottom reaches the last visible row.
  (C) collapse — clicking an expanded top section drops its CHILDREN_PER
      leaves from the visible sequence; the count falls, the next section
      slides into the top window, the section's leaves are gone.
  (D) re-expand — clicking it again restores the leaves and the boot window.
  (E) deep expand (AI-first) — scroll to a collapsed deep section, click to
      expand it, and its leaves become reachable though they never existed
      at boot.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the windowed rows really paint — a
      branch row renders glyph + label content (not a flat fill).
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
    wait_until,
)

EXAMPLE = "hello-virtual-tree"
WIN = (480, 520)
SECTIONS = 100
CHILDREN_PER = 99
EXPANDED_AT_BOOT = 4
TOTAL_NODES = SECTIONS * (1 + CHILDREN_PER)
PITCH = 48
OVERSCAN = 3
TREE_TAG = "vtree"
SCROLL_TAG = "vtree_scroll"
BOOT_EXPANDED = set(range(EXPANDED_AT_BOOT))


def flat_order(expanded: set[int]) -> list[str]:
    """Python mirror of `flat_visible` over the binding's dataset: the id
    at each visible row index, given which sections are expanded."""
    order: list[str] = []
    for s in range(SECTIONS):
        order.append(f"s{s}")
        if s in expanded:
            for i in range(CHILDREN_PER):
                order.append(f"s{s}-i{i}")
    return order


def visible_window(offset: int, vp_h: int, n: int, pitch: int, overscan: int) -> set[int]:
    """Python mirror of `compute_visible_range` (the windowing SSOT),
    fed the MEASURED viewport height read off the Scroll node."""
    if n == 0 or vp_h <= 0 or pitch == 0:
        return set()
    offset = max(0, offset)
    bottom = offset + vp_h
    max_index = n - 1
    first_visible = min(offset // pitch, max_index)
    last_visible = min((bottom - 1) // pitch, max_index)
    first = max(0, first_visible - overscan)
    last = min(last_visible + overscan, max_index)
    return set(range(first, last + 1))


def present_ids(snap) -> set[str]:
    out: set[str] = set()
    for tag in abs_rects_of(snap):
        if tag.startswith(f"{TREE_TAG}#"):
            out.add(tag.split("#", 1)[1])
    return out


def present_indices(snap, order: list[str]) -> set[int]:
    idx_of = {row_id: k for k, row_id in enumerate(order)}
    return {idx_of[row_id] for row_id in present_ids(snap) if row_id in idx_of}


def scroll_node(snap):
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return node


def scroll_offset(snap) -> int:
    return int(scroll_node(snap).get("offset_y", -1))


def scroll_viewport_h(snap) -> int:
    vp = scroll_node(snap).get("viewport") or {}
    return int(vp.get("h", -1))


def _walk_text(node, needle: str):
    if not isinstance(node, dict):
        return None
    if node.get("type") == "Text":
        content = node.get("content")
        return content if isinstance(content, str) and needle in content else None
    if node.get("type") == "Scroll":
        return _walk_text(node.get("content"), needle)
    for child in node.get("children") or []:
        found = _walk_text(child, needle)
        if found:
            return found
    return None


def header_visible(snap):
    """The AI-first visible-row count off the header status line (§2 #7
    scene-as-data) — `None` if the header has not rendered yet."""
    content = _walk_text(snap, "rows visible")
    if content is None:
        return None
    m = re.search(r"(\d+) rows visible", content)
    return int(m.group(1)) if m else None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot: small window over 496 visible rows ────────────
        # AutoSizer warmup: poll until the measured viewport is written
        # and the first window has rendered.
        snap = wait_until(
            lambda: (lambda s: s if present_ids(s) else None)(snap_now()),
            desc="boot window renders after AutoSizer warmup",
        )
        rects = abs_rects_of(snap)
        assert SCROLL_TAG in rects, "scroll container present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")

        vp_h = scroll_viewport_h(snap)
        assert vp_h > 0, f"Scroll clip height measured (>0), got {vp_h}"
        assert vp_h < WIN[1], f"clip is the flex band, smaller than the window, got {vp_h}"

        order = flat_order(BOOT_EXPANDED)
        assert_eq(len(order), SECTIONS + EXPANDED_AT_BOOT * CHILDREN_PER, "boot visible == 496")
        boot_window = visible_window(0, vp_h, len(order), PITCH, OVERSCAN)
        assert_eq(present_indices(snap, order), boot_window,
                  "boot band == windowing math fed the MEASURED viewport")
        rendered = len(present_ids(snap))
        assert rendered < 40, f"virtualized: small window, got {rendered} of {len(order)} visible"
        assert rendered >= 8, f"window covers the visible band, got {rendered}"
        assert "s0" in present_ids(snap), "top section rendered"
        assert "s99" not in present_ids(snap), "a deep section is NOT rendered (the whole point)"
        assert "s1" not in present_ids(snap), "section 1 is below the boot window (s0 + 99 leaves)"
        assert_eq(header_visible(snap), len(order), "header reports 496 visible rows (AI-first)")

        # Rows are one pitch apart top to bottom.
        y0 = rects[f"{TREE_TAG}#s0"][1]
        y1 = rects[f"{TREE_TAG}#s0-i0"][1]
        assert_eq(y1 - y0, PITCH, "adjacent rows are exactly one pitch apart")

        # ── (B) the window slides ───────────────────────────────────
        tf.scroll(SCROLL_TAG, to=(0, 200 * PITCH))
        snap = wait_until(
            lambda: (lambda s: s if scroll_offset(s) == 200 * PITCH else None)(snap_now()),
            desc="scroll-to landed at offset 200*pitch",
        )
        mid_window = visible_window(200 * PITCH, vp_h, len(order), PITCH, OVERSCAN)
        assert_eq(present_indices(snap, order), mid_window,
                  "scrolled band == windowing math at the new offset")
        assert "s0" not in present_ids(snap), "top section scrolled out"
        assert len(present_ids(snap)) < 40, "window stays small mid-scroll"

        # Scroll to the very bottom — clamps to len*pitch - measured vp.
        tf.scroll(SCROLL_TAG, to=(0, 10 ** 9))
        max_off = len(order) * PITCH - vp_h
        snap = wait_until(
            lambda: (lambda s: s if scroll_offset(s) == max_off else None)(snap_now()),
            desc="offset clamps to len*pitch - measured_viewport (sizer drove max_y)",
        )
        assert "s99" in present_ids(snap), "the last section is reachable at the bottom"
        assert len(present_ids(snap)) < 40, "window small even at the bottom"

        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_until(
            lambda: (lambda s: s if scroll_offset(s) == 0 else None)(snap_now()),
            desc="scrolled back to top",
        )
        assert_eq(present_indices(snap, order), boot_window, "top window restored exactly")

        # ── (C) collapse a top section ──────────────────────────────
        tf.click(path=f"{TREE_TAG}#s0")
        collapsed_order = flat_order(BOOT_EXPANDED - {0})
        assert_eq(len(collapsed_order), SECTIONS + (EXPANDED_AT_BOOT - 1) * CHILDREN_PER,
                  "collapsing s0 drops CHILDREN_PER from the visible count")
        snap = wait_until(
            lambda: (lambda s: s if header_visible(s) == len(collapsed_order) else None)(snap_now()),
            desc="collapse s0 → header visible count falls by CHILDREN_PER",
        )
        assert "s0-i0" not in present_ids(snap), "s0's leaves left the visible sequence"
        assert "s1" in present_ids(snap), "s1 slid into the top window after s0 collapsed"
        assert_eq(present_indices(snap, collapsed_order),
                  visible_window(0, vp_h, len(collapsed_order), PITCH, OVERSCAN),
                  "post-collapse band == windowing math over the shorter sequence")

        # ── (D) re-expand restores the boot window ──────────────────
        tf.click(path=f"{TREE_TAG}#s0")
        snap = wait_until(
            lambda: (lambda s: s if header_visible(s) == len(order) else None)(snap_now()),
            desc="re-expand s0 → header visible count restored to 496",
        )
        assert "s0-i0" in present_ids(snap), "s0's leaves are back in the top window"
        assert_eq(present_indices(snap, order), boot_window, "boot window restored after re-expand")

        # ── (E) deep expand (AI-first) — reveal s50's leaves ────────
        # s50 is collapsed at boot; scroll until it is in the window, click
        # to expand, and its leaves become reachable though they never
        # existed at boot.
        s50_index = order.index("s50")
        tf.scroll(SCROLL_TAG, to=(0, s50_index * PITCH))
        snap = wait_until(
            lambda: (lambda s: s if "s50" in present_ids(s) else None)(snap_now()),
            desc="scrolled s50 into the window",
        )
        assert "s50-i0" not in present_ids(snap), "s50's leaves do not exist before expand"
        tf.click(path=f"{TREE_TAG}#s50")
        expanded_order = flat_order(BOOT_EXPANDED | {50})
        snap = wait_until(
            lambda: (lambda s: s if header_visible(s) == len(expanded_order) else None)(snap_now()),
            desc="expand s50 → visible count grows by CHILDREN_PER",
        )
        assert_eq(len(expanded_order), len(order) + CHILDREN_PER, "s50 expand added CHILDREN_PER")
        assert "s50-i0" in present_ids(snap), "s50's first leaf is now reachable in the window"
        assert len(present_ids(snap)) < 40, "still a window after the deep expand"

    # ── Phase 2 — live pixels (boot frame: a branch row paints) ─────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    # Section row s0 (a branch) renders an expand glyph + label, so a
    # horizontal sweep across it is NOT a flat fill.
    x, y, w, h = rects[f"{TREE_TAG}#s0"]
    pts = [(x + dx, y + h // 2) for dx in range(6, w - 6, 6)]
    cols = sample_png_points(img, pts)
    assert len(set(cols)) >= 2, \
        f"branch row s0 renders glyph + label content (not a flat fill), got {set(cols)}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = wait_until(
            lambda: (lambda s: s if any(t.startswith(f"{TREE_TAG}#") for t in abs_rects_of(s)) else None)(
                tf.snapshot(source="paint", viewport=WIN)
            ),
            desc="boot window renders for the pixel pass",
        )
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r819-")) / "vtree.png"
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
    sys.exit(run_demo("R819 §5.27 — virtualized TreeView", body))
