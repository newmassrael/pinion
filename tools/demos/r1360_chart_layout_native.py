#!/usr/bin/env python3
"""R1360 layout-native chart demo — RPC introspection + live-pixel.

The chart stopped being pinned to a `const CHART_RECT`. `hello-chart`
resolves its whole geometry against a compile-time rect handed to
`LineChart::build` before layout runs, so it cannot flex, dock, or respond
to a resize. R1358 removed the *primitive* blocker (a `Scene::Path`'s
commands became relative to its own rect, so a path is placed by layout);
R1360 added `LineChart::build_fill` — children authored in the chart's own
`(0,0)..(w,h)` frame under a fill-parent root — plus the measured-rect seam
that feeds the slot's size back to the view.

Proven here by typed RPC rather than by asking a human to describe a
screenshot — `rpc_verify`'s stated obligation, and the discipline whose
absence is exactly why R1360.1 shipped a black-chrome bug that four unit
tests and every PNG missed.

  Phase 1 — THE CLAIM ITSELF (real `scene/resize`, per the r774 precedent).
    The window is actually grown over the wire and the chart must follow:
    root, painted body, and the size the view reports all track the new
    slot. `hello-chart` structurally cannot do this — its chart is pinned to
    a compile-time `const CHART_RECT`.
    Also asserted at every size: the seam's FIXED POINT — the size the view
    says it built at (the status line) equals the rect layout produced for
    the chart root. A stale, oscillating, or ignored seam breaks that
    equality even when the root alone still tracks (a fill-parent root
    tracks its slot however stale the body inside it is — which is exactly
    how a hardcoded constant slipped past 3 of this binding's 4 unit tests
    until `body_extent` was added).
    The sweep is ASCENDING because this WM honours grow but not shrink —
    the same constraint r774 documents; shrink is covered by the binding's
    unit tests, which drive the loop directly.

  Phase 2 — CHROME CONTRAST (the R1360.1 regression). The binding first
    shipped painting no background: every pixel outside the chart body was
    RGBA(0,0,0,0), which a compositor renders black under text coloured for
    a light surface. A human's eyes caught it. The unit tests were blind
    because they only asked about geometry — but the screenshot was NOT
    blind: the capture preserves alpha (`encode_rgba8_png` writes
    `ColorType::Rgba`), so the zero was in the PNG all along and only an
    image *viewer* composites it onto white. Phase 3 below is that missing
    sample; this phase reads the same fact as DATA over the wire, so the
    defect cannot return silently by either route.

  Phase 3 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The assertion R1360.1 needed and did not have.

One dead end worth recording, since a first cut of this demo fell into it:
`scene/snapshot`'s `viewport` argument does NOT re-lay-out an app that
already has a window — it reports the live-window scene (verified: passing
430x300 and 1000x700 to `hello-chart` returns its 760x460 root either way).
Driving the geometry means resizing the real window with `scene/resize`,
which is what r774 does for the sibling AutoSizer seam.

Run from the workspace root:
    cargo build -p hello-chart-fill --release
    python3 tools/demos/r1360_chart_layout_native.py
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    WORKSPACE_ROOT,
    assert_eq,
    assert_pixel_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
    wait_until,
)

VIEWPORT = (720, 420)
# Ascending: this WM honours grow, not shrink (the constraint r774 records).
GROW_TO = [(820, 500), (940, 580)]
CHART_TAG = "chart"

# Chrome geometry the binding declares (TITLE_H / STATUS_H / padding). The
# chart slot is what is left over — asserted as derived, never hardcoded.
PAD = 10


def _node(snap, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in the paint snapshot"
    return node


def _rect(node: dict) -> tuple[int, int, int, int]:
    r = node["rect"]
    return (r["x"], r["y"], r["w"], r["h"])


def _status_text(snap) -> str:
    """The status line, which prints the size the view was handed."""
    found: list[str] = []

    def walk(n: dict) -> None:
        if n.get("type") == "Text" and "measured" in str(n.get("content", "")):
            found.append(n["content"])
        for c in n.get("children") or []:
            walk(c)

    walk(snap)
    assert found, "the status line reports the measured slot size"
    return found[0]


def _body_extent(snap) -> tuple[int, int, int, int]:
    """The painted chart BODY's bbox in window px: the union of the x-axis
    and the series — geometry `build_fill` derived from the size it was
    handed. The chart ROOT is fill-parent, so its rect tracks its slot even
    if the body inside were built at a stale size; only the body proves the
    measured size reached the builder."""
    l = t = r = b = None
    for tag in ("chart.axis.x", "chart.series.0", "chart.series.1"):
        node = find_by_tag(snap, tag)
        if node is None:
            continue
        x, y, w, h = _rect(node)
        l = x if l is None else min(l, x)
        t = y if t is None else min(t, y)
        r = (x + w) if r is None else max(r, x + w)
        b = (y + h) if b is None else max(b, y + h)
    assert l is not None, "a measured chart paints an axis + series"
    return (l, t, r, b)


def _assert_coherent_at(snap, label: str) -> tuple[int, int, int, int]:
    """Every invariant that must hold at ANY window size. Returns the chart
    root's rect."""
    win = snap["rect"]
    cx, cy, cw, ch = _rect(_node(snap, CHART_TAG))

    # The chart root is DERIVED from the window, not a constant: it sits at
    # the padding inset and spans the padded width. A const-rect chart
    # (hello-chart) has no such relationship to its window at any size.
    assert_eq(cx, PAD, f"{label}: chart root x = the root's padding inset")
    assert_eq(cw, win["w"] - 2 * PAD, f"{label}: chart root fills the padded width")
    assert 0 < ch < win["h"], f"{label}: chart root height {ch} inside window {win}"

    # THE FIXED POINT: the size the view says it BUILT at is the size layout
    # MEASURED for the root. A stale / ignored / oscillating seam breaks this
    # even while the fill-parent root still tracks its slot.
    status = _status_text(snap)
    m = re.search(r"measured (\d+) x (\d+) px", status)
    assert m, f"{label}: status line reports a measured size, got {status!r}"
    said = (int(m.group(1)), int(m.group(2)))
    assert_eq(said, (cw, ch), f"{label}: the seam settled (built size == measured root)")

    # The body was built for THIS slot: inside the root, and spanning it.
    bl, bt, br, bb = _body_extent(snap)
    assert bl >= cx and bt >= cy, f"{label}: body ({bl},{bt}) starts inside root ({cx},{cy})"
    assert br <= cx + cw + 1 and bb <= cy + ch + 1, (
        f"{label}: body right/bottom ({br},{bb}) stays inside the root "
        f"({cx + cw},{cy + ch}) — an overflow means it was built at a size "
        f"the root does not have"
    )
    assert br - bl > cw // 2, f"{label}: body spans its slot ({br - bl} of {cw}px)"

    # Real data, not a placeholder: 24 buckets per series.
    for i in range(2):
        s = _node(snap, f"chart.series.{i}")
        assert_eq(len(s["commands"]), 24, f"{label}: series {i} = MoveTo + 23 LineTo")
    return (cx, cy, cw, ch)


def body() -> None:
    # ── Phase 1 — the chart tracks a REAL resize ─────────────────────
    with RpcSubprocess("hello-chart-fill", boot_grace=1.5) as d:

        def snap_now():
            return d.snapshot(source="paint", viewport=VIEWPORT)

        def grow_to(w: int, h: int):
            """Drive a real `scene/resize`, then poll `from=paint` until the
            stored frame reflects the new window (resize event + repaint +
            the seam's publish/re-view all settled). Zero-flake by outcome,
            not by sleeping — the r774 shape."""
            resp = d.request("scene/resize", {"width": w, "height": h})
            assert resp is not None and resp.result is not None, "scene/resize accepted"

            def settled():
                s = snap_now()
                return s if s["rect"]["w"] == w else None

            return wait_until(settled, desc=f"window settles at {w}x{h} after resize")

        boot = snap_now()
        _, _, boot_w, boot_h = _assert_coherent_at(boot, "boot")

        prev_w, prev_h = boot_w, boot_h
        prev_body = _body_extent(boot)
        for (w, h) in GROW_TO:
            snap = grow_to(w, h)
            _, _, cw, ch = _assert_coherent_at(snap, f"{w}x{h}")
            # The claim: a bigger window means a bigger chart — root AND the
            # painted geometry inside it.
            assert cw > prev_w and ch > prev_h, (
                f"the chart root grew with the window: {prev_w}x{prev_h} -> {cw}x{ch}"
            )
            body_now = _body_extent(snap)
            assert (body_now[2] - body_now[0]) > (prev_body[2] - prev_body[0]), (
                f"the PAINTED geometry grew too: body width "
                f"{prev_body[2] - prev_body[0]} -> {body_now[2] - body_now[0]}. "
                f"Equal widths mean the view ignored the measured size and the "
                f"fill-parent root tracked the slot around a stale body"
            )
            prev_w, prev_h, prev_body = cw, ch, body_now

        # ── Phase 2 — the R1360.1 chrome-contrast regression ─────────
        fill = snap_now()["style"]["fill"]
        assert_eq(fill["a"], 255, (
            "the window root paints an OPAQUE surface — a transparent root "
            "leaves the title/status chrome on the compositor's black, under "
            "text coloured for a light surface"
        ))
        assert (fill["r"], fill["g"], fill["b"]) != (0, 0, 0), (
            f"the surface is a real colour, not black: {fill}"
        )

    # ── Phase 3 — live pixels: the chrome is the opaque surface ──────
    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert (img.width, img.height) == VIEWPORT, (
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    )
    # The exact pixels that were RGBA(0,0,0,0) before R1360.1.
    title_px, corner_px = sample_png_points(img, [(400, 12), (4, 4)])
    for name, px in (("title row (400,12)", title_px), ("padding (4,4)", corner_px)):
        assert_eq(px[3], 255, f"{name} is opaque (alpha 255)")
        assert px[:3] != (0, 0, 0), f"{name} is the surface, not black: {px}"
    assert_pixel_eq(
        corner_px, title_px, "the padding matches the title row's surface", tolerance=2
    )


def capture_screenshot() -> Path:
    """Render the boot frame to RGBA8 PNG via `PINION_SCREENSHOT`, bypassing
    winit. Producer parity: the same `to_vello_cached` rasterizer the live
    window uses, so an alpha=0 chrome pixel here is the one a compositor
    would paint black. The frame size is the binding's own
    `initial_size_strategy`, not a demo-side override."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r1360-")) / "chart_fill.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-chart-fill"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-chart-fill", "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n"
            f"  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"screenshot not written to {out}")
    return out


if __name__ == "__main__":
    run_demo("R1360 layout-native chart (fills its slot)", body)
