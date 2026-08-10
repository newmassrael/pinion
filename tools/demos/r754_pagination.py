#!/usr/bin/env python3
"""R754 §5.38 §5.40 Material 3 pagination (`hello-pagination`).

A pager of five numbered page links with clamping previous / next
chevrons. The page cells reuse the `RadioGroupExternal` machinery through
the `PaginationExternal` coordinator (single-select current page, per-cell
state, the §5.20 `"selected"` intent); previous / next *step* one page and
*clamp* at the ends (no previous on page 1, no next on the last page).

This is the **3rd consumer** of the lifted `navigation_link_nodes` a11y
substrate (after `hello-breadcrumb` and `hello-nav-rail`): a `navigation`
landmark of `link`s, the current page carrying `aria-current="page"`, with
previous / next as `aria-disabled` end links.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot: page 3 (index 2) current; both prev and next available;
  * a current page paints an opaque Accent circle; others are transparent;
  * `scene/click` on a page navigates to it (1-of-N current moves);
  * clicking `next` / `prev` steps one page and clamps at the ends;
  * `can_prev` / `can_next` track the ends;
  * keyboard: arrows step and CLAMP (no wrap), Home / End jump.

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame; source of truth = the
paint scene's own fill colours):
  * the current page renders the opaque Accent the snapshot reports;
  * the window background is Surface.

Run from the workspace root:
    cargo build -p hello-pagination --release
    python3 tools/demos/r754_pagination.py
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    assert_pixel_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-pagination"
VIEWPORT = (460, 120)
NAV = "pagination"
N = 5


def cell(i: int) -> str:
    return f"{NAV}#{i}"


def cur(d):
    return d.query("/external/selected_index")


def paint(d):
    return d.snapshot(source="paint", viewport=VIEWPORT)


def cell_fill(snap, i: int):
    node = find_by_tag(snap, cell(i))
    assert node is not None, f"page {i} present"
    f = node["style"]["fill"]
    return (f["r"], f["g"], f["b"], f.get("a", 255))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as d:
        # ── boot: page index 2 current; prev + next both available ───────
        assert_eq(cur(d), 2, "boot current = page 3 (index 2)")
        assert_eq(d.query("/external/selected.2"), True, "page 2 current")
        assert_eq(d.query("/external/selected.0"), False, "page 0 not current")
        assert_eq(d.query("/external/selected.4"), False, "page 4 not current")
        assert_eq(d.query("/external/can_prev"), True, "mid page: has previous")
        assert_eq(d.query("/external/can_next"), True, "mid page: has next")
        assert_eq(d.query("/external/count"), N, "five pages")

        snap = paint(d)
        assert find_by_tag(snap, NAV) is not None, "pager row tagged pagination"
        for i in range(N):
            assert find_by_tag(snap, cell(i)) is not None, f"page {i} present"
        assert find_by_tag(snap, f"{NAV}#prev") is not None, "prev chevron present"
        assert find_by_tag(snap, f"{NAV}#next") is not None, "next chevron present"
        accent = cell_fill(snap, 2)
        assert accent[3] == 255, f"current page 2 opaque ({accent})"
        assert cell_fill(snap, 0)[3] == 0, "non-current page 0 transparent"

        rects = abs_rects_of(snap)
        assert NAV in rects, "pager row has an absolute rect"
        surface = snap["style"]["fill"]
        surface_rgb = (surface["r"], surface["g"], surface["b"])

        # ── click page 4 (last): becomes current, next clamps off ────────
        d.click(path=cell(4))
        d.pointer_leave()
        assert_eq(cur(d), 4, "click navigates to page 4")
        assert_eq(d.query("/external/selected.4"), True, "page 4 now current")
        assert_eq(d.query("/external/selected.2"), False, "page 2 no longer current")
        assert_eq(d.query("/external/can_next"), False, "last page: no next")
        assert_eq(d.query("/external/can_prev"), True, "last page: has previous")
        assert cell_fill(paint(d), 4)[3] == 255, "page 4 now opaque Accent"

        # ── click next on the last page: clamp (no-op) ───────────────────
        #
        # R1627 — this assertion USED TO PASS FOR THE WRONG REASON. R1619 grew
        # the send wire by a segment and both chevrons went silently dead, so
        # "next did nothing" was true because next did nothing EVER. A no-op
        # assertion cannot tell a clamp from a corpse, so it is now paired
        # with a click in the direction that must move — and the pair is
        # ordered so the moving one runs first, which is what makes the
        # standing-still one mean something.
        d.click(path=f"{NAV}#prev")
        d.pointer_leave()
        assert_eq(cur(d), 3, "the chevron is alive: prev steps 4 -> 3")
        d.click(path=f"{NAV}#next")
        d.pointer_leave()
        assert_eq(cur(d), 4, "and next steps back 3 -> 4")
        d.click(path=f"{NAV}#next")
        d.pointer_leave()
        assert_eq(cur(d), 4, "next on the last page clamps (no-op)")

        # ── click prev: steps back one page ──────────────────────────────
        d.click(path=f"{NAV}#prev")
        d.pointer_leave()
        assert_eq(cur(d), 3, "prev steps 4 -> 3")
        assert_eq(d.query("/external/can_next"), True, "and next is available again")
        assert_eq(d.query("/external/can_next"), True, "next available again")

        # ── click page 0: current moves; prev clamps off ────────────────
        d.click(path=cell(0))
        d.pointer_leave()
        assert_eq(cur(d), 0, "click navigates to page 0")
        assert_eq(d.query("/external/can_prev"), False, "first page: no previous")

        # ── click prev on the first page: clamp (no-op) ──────────────────
        d.click(path=f"{NAV}#prev")
        d.pointer_leave()
        assert_eq(cur(d), 0, "prev on the first page clamps (no-op)")

        # ── keyboard: arrows step and CLAMP (no wrap), Home/End jump ──────
        focused = d.request("focus/set", {"tag": NAV}).result.get("focused")
        assert_eq(focused, NAV, "pager is the single tab stop")
        d.key(path=NAV, name="ArrowLeft")
        assert_eq(cur(d), 0, "ArrowLeft on page 0 clamps (no wrap)")
        d.key(path=NAV, name="ArrowRight")
        assert_eq(cur(d), 1, "ArrowRight 0 -> 1")
        d.key(path=NAV, name="ArrowRight")
        assert_eq(cur(d), 2, "ArrowRight 1 -> 2")
        d.key(path=NAV, name="ArrowLeft")
        assert_eq(cur(d), 1, "ArrowLeft 2 -> 1")
        d.key(path=NAV, name="End")
        assert_eq(cur(d), 4, "End -> last page")
        d.key(path=NAV, name="ArrowRight")
        assert_eq(cur(d), 4, "ArrowRight on the last page clamps (no wrap)")
        d.key(path=NAV, name="Home")
        assert_eq(cur(d), 0, "Home -> first page")

    # ── Phase 2 — live-pixel (boot frame: page index 2 current) ──────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    cx, cy, cw, _ch = rects[cell(2)]            # current page circle
    mid_current = (cx + cw // 2, cy + 6)        # above the centred numeral
    corner = (5, 5)                             # window background (Surface)
    p_current, p_corner = sample_png_points(png, [mid_current, corner])
    assert_pixel_eq(p_current, (*accent[:3], 255), f"current page Accent {mid_current}", tolerance=12)
    assert_pixel_eq(p_corner, (*surface_rgb, 255), f"window surface {corner}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r754-")) / "pagination.png"
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
    sys.exit(run_demo("R754 Material 3 pagination", body))
