#!/usr/bin/env python3
"""R820 §5.16 §5.27 §5.50 — virtualized TreeView keyboard E2E.

Drives the `hello-virtual-tree` binding via JSON-RPC. R819 landed the
windowed paint (`view_virtual_tree`); R820 adds WAI-ARIA APG 6.13 tree
keyboard navigation over the window, composing three lifted SSOTs with no
new substrate logic: the `apply_tree_key` resolve→flag-store bridge
(R820, shared with `hello-tree-view`), `scroll_offset_to_reveal` (R774
scroll-into-view), and `clamp_nav` (R777 finite-collection motion).

The decisive witness is the keyboard ⊥ virtualization composition: driving
the cursor past the rendered window **scrolls the off-window target into
view and materializes it** — the row never existed as a scene node until
the cursor reached it. Expand / collapse via Arrow keys changes the visible
sequence; Home / End jump to the ends; type-ahead jumps by label.

Atomic verification scope (>=30 assertions):

  (A) boot — windowed, no cursor yet; the tree root is the single tab stop.
  (B) Arrow Down/Up — the cursor moves one visible row at a time and clamps
      at the first row (no wrap).
  (C) keyboard ⊥ virtualization — driving the cursor far down scrolls the
      window so the cursor row is always rendered (and carries the focus
      fill); the boot-top rows have left the window.
  (D) Arrow Right / Left — expand a collapsed section / collapse it; the
      visible-row count tracks the change.
  (E) Home / End — jump the cursor + window to the first / last visible row.
  (F) type-ahead — a letter jumps the cursor to the next matching label and
      scrolls it into view.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the focused row paints the M3
      focus state-layer (distinct from the unfocused rows).
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
    RpcSubprocess,
    WORKSPACE_ROOT,
    unclipped_rects_of,
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
PITCH = 48
OVERSCAN = 3
TREE_TAG = "vtree"
ROOT_TAG = "vtree_root"
SCROLL_TAG = "vtree_scroll"
AT = (12.0, 60.0)  # any in-window point; named keys route to the focused tree
BOOT_EXPANDED = set(range(EXPANDED_AT_BOOT))


def flat_order(expanded: set[int]) -> list[str]:
    order: list[str] = []
    for s in range(SECTIONS):
        order.append(f"s{s}")
        if s in expanded:
            for i in range(CHILDREN_PER):
                order.append(f"s{s}-i{i}")
    return order


def visible_window(offset: int, vp_h: int, n: int, pitch: int, overscan: int) -> set[int]:
    if n == 0 or vp_h <= 0 or pitch == 0:
        return set()
    offset = max(0, offset)
    bottom = offset + vp_h
    max_index = n - 1
    first_visible = min(offset // pitch, max_index)
    last_visible = min((bottom - 1) // pitch, max_index)
    return set(range(max(0, first_visible - overscan), min(last_visible + overscan, max_index) + 1))


def _walk(node, fn):
    if not isinstance(node, dict):
        return
    fn(node)
    if node.get("type") == "Scroll":
        _walk(node.get("content"), fn)
    for child in node.get("children") or []:
        _walk(child, fn)


def present_ids(snap) -> set[str]:
    out: set[str] = set()
    _walk(snap, lambda n: out.add(n["tag"].split("#", 1)[1])
          if isinstance(n.get("tag"), str) and n["tag"].startswith(f"{TREE_TAG}#") else None)
    return out


def present_indices(snap, order: list[str]) -> set[int]:
    idx_of = {row_id: k for k, row_id in enumerate(order)}
    return {idx_of[r] for r in present_ids(snap) if r in idx_of}


def cursor_id(snap):
    """Id of the row carrying the M3 focus-highlight fill (non-transparent
    background) — the keyboard cursor row; `None` when nothing is focused."""
    found = []

    def visit(n):
        if n.get("type") != "Container":
            return
        tag = n.get("tag")
        if not (isinstance(tag, str) and tag.startswith(f"{TREE_TAG}#")):
            return
        fill = (n.get("style") or {}).get("fill") or {}
        if int(fill.get("a") or 0) != 0:
            found.append(tag.split("#", 1)[1])

    _walk(snap, visit)
    return found[0] if found else None


def scroll_node(snap):
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return node


def scroll_offset(snap) -> int:
    return int(scroll_node(snap).get("offset_y", -1))


def scroll_viewport_h(snap) -> int:
    return int((scroll_node(snap).get("viewport") or {}).get("h", -1))


def _walk_text(node, needle: str):
    if not isinstance(node, dict):
        return None
    if node.get("type") == "Text":
        c = node.get("content")
        return c if isinstance(c, str) and needle in c else None
    if node.get("type") == "Scroll":
        return _walk_text(node.get("content"), needle)
    for child in node.get("children") or []:
        r = _walk_text(child, needle)
        if r:
            return r
    return None


def header_visible(snap):
    content = _walk_text(snap, "rows visible")
    if content is None:
        return None
    import re
    m = re.search(r"(\d+) rows visible", content)
    return int(m.group(1)) if m else None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        def press(name: str):
            tf.key(at=AT, name=name)

        # ── (A) boot: windowed, no cursor, single tab stop ──────────
        snap = wait_until(
            lambda: (lambda s: s if present_ids(s) else None)(snap_now()),
            desc="boot window renders",
        )
        assert ROOT_TAG in unclipped_rects_of(snap), "focusable tree root present"
        assert_eq(scroll_offset(snap), 0, "boot offset 0")
        # The tree is a single tab stop, so the cursor boots on the first row
        # (WAI-ARIA aria-activedescendant defined from frame one).
        assert_eq(cursor_id(snap), "s0", "boot cursor on the first row")
        vp_h = scroll_viewport_h(snap)
        assert vp_h > 0, "measured viewport set"
        order = flat_order(BOOT_EXPANDED)
        assert "s0" in present_ids(snap), "top section rendered at boot"
        assert_eq(present_indices(snap, order), visible_window(0, vp_h, len(order), PITCH, OVERSCAN),
                  "boot band == windowing math at the measured viewport")
        assert "s99" not in present_ids(snap), "a deep section is NOT rendered at boot"
        assert len(present_ids(snap)) < 40, "virtualized: small window over many visible rows"

        # The tree is a single tab stop with a focus gate (apply_key returns
        # false unless ROOT_TAG is focused), so focus it before driving keys
        # — the convention every gated single-widget demo follows.
        tf.request("focus/set", {"tag": ROOT_TAG})

        # ── (B) Arrow Down/Up move + clamp ──────────────────────────
        # Up at the first row clamps (no wrap to the last row).
        press("ArrowUp")
        snap = snap_now()
        assert_eq(cursor_id(snap), "s0", "ArrowUp clamps at the first row (no wrap)")
        press("ArrowDown")  # s0 -> s0-i0
        wait_until(lambda: cursor_id(snap_now()) == "s0-i0", desc="ArrowDown -> s0-i0")
        snap = snap_now()
        assert_eq(scroll_offset(snap), 0, "near-top cursor needs no scroll")
        # PageDown jumps a viewport-ful (clamped) — a multi-row advance.
        press("PageDown")
        snap = wait_until(
            lambda: (lambda s: s if cursor_id(s) not in ("s0", "s0-i0") else None)(snap_now()),
            desc="PageDown advances the cursor multiple rows",
        )

        # ── (C) keyboard ⊥ virtualization: drive past the window ────
        boot_top = present_ids(snap_now())
        for _ in range(40):
            press("ArrowDown")
        snap = wait_until(
            lambda: (lambda s: s if scroll_offset(s) > 0 else None)(snap_now()),
            desc="driving the cursor down scrolled the window",
        )
        cur = cursor_id(snap)
        assert cur is not None, "cursor still set deep in the list"
        # The cursor row was scrolled into view and is rendered + focused.
        assert cur in present_ids(snap), "cursor row materialized in the window"
        cur_idx = order.index(cur)
        win = visible_window(scroll_offset(snap), vp_h, len(order), PITCH, OVERSCAN)
        assert cur_idx in win, f"cursor index {cur_idx} inside the revealed window {sorted(win)[:3]}.."
        assert_eq(present_indices(snap, order), win, "deep band == windowing math at the offset")
        assert "s0" not in present_ids(snap), "boot-top section scrolled out of the window"
        assert len(present_ids(snap)) < 40, "window stays small after deep navigation"

        # ── (D) Arrow Right / Left expand + collapse ────────────────
        press("Home")  # back to s0 (expanded section), window to top
        wait_until(lambda: cursor_id(snap_now()) == "s0", desc="Home -> cursor s0")
        snap = snap_now()
        assert_eq(scroll_offset(snap), 0, "Home scrolled to the top")
        assert_eq(header_visible(snap), len(order), "boot visible count restored at top")
        # ArrowLeft on the expanded s0 collapses it.
        press("ArrowLeft")
        collapsed = flat_order(BOOT_EXPANDED - {0})
        snap = wait_until(
            lambda: (lambda s: s if header_visible(s) == len(collapsed) else None)(snap_now()),
            desc="ArrowLeft collapses s0 (visible count falls)",
        )
        assert "s0-i0" not in present_ids(snap), "s0's leaves left the visible sequence"
        assert "s1" in present_ids(snap), "s1 slid into the top window after s0 collapsed"
        assert_eq(present_indices(snap, collapsed),
                  visible_window(0, vp_h, len(collapsed), PITCH, OVERSCAN),
                  "post-collapse band == windowing math over the shorter sequence")
        # ArrowRight on the collapsed s0 re-expands it.
        press("ArrowRight")
        snap = wait_until(
            lambda: (lambda s: s if header_visible(s) == len(order) else None)(snap_now()),
            desc="ArrowRight re-expands s0",
        )
        assert "s0-i0" in present_ids(snap), "s0's leaves are back"

        # ── (E) Home / End jump to the ends ─────────────────────────
        press("End")
        snap = wait_until(
            lambda: (lambda s: s if cursor_id(s) == "s99" else None)(snap_now()),
            desc="End -> cursor on the last row s99",
        )
        assert "s99" in present_ids(snap), "End scrolled the last section into view"
        assert scroll_offset(snap) > 0, "End scrolled away from the top"
        assert_eq(scroll_offset(snap), len(order) * PITCH - vp_h,
                  "End clamps to len*pitch - measured_viewport (sizer drove max_y)")
        # ArrowDown at the last row clamps (no wrap to the top).
        press("ArrowDown")
        snap = snap_now()
        assert_eq(cursor_id(snap), "s99", "ArrowDown clamps at the last row (no wrap)")
        press("Home")
        snap = wait_until(
            lambda: (lambda s: s if cursor_id(s) == "s0" else None)(snap_now()),
            desc="Home -> cursor back on s0",
        )
        assert_eq(scroll_offset(snap), 0, "Home returned to the top")
        assert_eq(present_indices(snap, order), visible_window(0, vp_h, len(order), PITCH, OVERSCAN),
                  "top window restored exactly after the End/Home round-trip")

        # ── (F) type-ahead jumps by label ───────────────────────────
        # Labels: sections are "Section NNN", leaves "Item NNN-NNNN". From
        # s0, typing 'i' jumps to the next "Item ..." row (a leaf).
        press("i")
        snap = wait_until(
            lambda: (lambda s: (lambda c: s if c and "-i" in c else None)(cursor_id(s)))(snap_now()),
            desc="type-ahead 'i' jumps to an Item leaf",
        )
        cur = cursor_id(snap)
        assert cur != "s0", "type-ahead moved the cursor off the section row"
        assert cur in present_ids(snap), "type-ahead target scrolled into view"

    # ── Phase 2 — live pixels: the focused row paints a highlight ────
    snap, rects = _focused_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"
    cur = cursor_id(snap)
    assert cur is not None, "a row is focused in the screenshot frame"

    def fully_visible(tag: str) -> bool:
        # The virtualization window deliberately renders OVERSCAN rows
        # past the viewport edges, so `present_ids` can include a row
        # whose rect hangs below the fold — sampling its centre lands
        # outside the screenshot (the CI 27268693324 / cold-boot flake:
        # y ≈ 541 vs the 520 px window). Pixel contrast rows must be
        # fully inside the captured frame.
        x, y, w, h = rects[f"{TREE_TAG}#{tag}"]
        return x >= 0 and y >= 0 and x + w <= WIN[0] and y + h <= WIN[1]

    assert fully_visible(cur), "the boot cursor row (s0) sits fully in-frame"
    fx, fy, fw, fh = rects[f"{TREE_TAG}#{cur}"]
    # An unfocused sibling row to contrast against — fully in-frame, so
    # an overscan row at the fold can never be picked.
    other = next(t for t in present_ids(snap) if t != cur and fully_visible(t))
    ox, oy, ow, oh = rects[f"{TREE_TAG}#{other}"]
    foc, unf = sample_png_points(img, [(fx + fw - 6, fy + fh // 2), (ox + ow - 6, oy + oh // 2)])
    assert foc != unf, f"focused row paints a distinct highlight, got {foc} vs {unf}"


def _focused_snapshot_and_rects():
    # The boot frame already has the cursor on s0 (single tab stop), so the
    # screenshot's boot frame paints the focus highlight — no key needed.
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = wait_until(
            lambda: (lambda s: s if cursor_id(s) == "s0" else None)(tf.snapshot(source="paint", viewport=WIN)),
            desc="boot cursor on s0 for the pixel pass",
        )
        return snap, unclipped_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r820-")) / "vtree-kbd.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", EXAMPLE, "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    # The binding boots with the cursor on s0, so the screenshot's boot frame
    # paints the focus highlight on that row (contrasted against an unfocused
    # sibling in the pixel assertion).
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
    sys.exit(run_demo("R820 §5.27 — virtualized TreeView keyboard", body))
