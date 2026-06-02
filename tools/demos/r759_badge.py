#!/usr/bin/env python3
"""R759 §5.38 §5.40 Badge (`hello-badge`).

A descriptive (non-interactive) **count / dot status overlay** anchored to
an element, shown in its two Material 3 variants side by side:

  * a **count** badge — a small error-tier pill carrying a number — over
    an "Inbox" anchor, demonstrating the `"{max}+"` overflow form;
  * a **dot** badge — a bare dot, "something new" — over a "Sync" anchor.

A badge owns no interaction statechart (R718 descriptive-widget pattern):
it is a plain `BadgeExternal` value holder whose axes (`count`, `max`,
`dot`, plus the derived read-only `label` / `visible`) are driven through
the §5.15 introspect channel. The two externals are composed through the
R55.D.5 `create_extra_externals` slot (the `hello-card` mould). The badge
sits at the anchor's top-right corner via the existing
`LayoutStyle::with_absolute_position` field; the pill / dot paint is built
inline (1st badge-paint consumer) over the M3 `error` colour tier.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot: a "3" count pill + a dot, both error-tier red, both reporting
    `visible = true` — the badge state is observable without pixels;
  * the count `label` caps at the overflow form `"99+"` while the raw
    `count` stays uncapped (`intervene count = 150`);
  * a count of 0 hides the count badge (`visible = false`, no pill in the
    paint scene) — the M3 / web-platform behaviour;
  * the dot badge re-purposes to a count pill when its `dot` flag is
    cleared and a count is set (the two variants share one holder shape);
  * a custom `max` moves the overflow point.

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame; source of truth = the
paint scene's own fills, so the assertion is introspection<->screen
parity, not a hardcoded palette guess):
  * the window background is Surface;
  * the count pill + the dot interiors are both the error-tier red.

Run from the workspace root:
    cargo build -p hello-badge --release
    python3 tools/demos/r759_badge.py
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

EXAMPLE = "hello-badge"
VIEWPORT = (360, 200)
COUNT_TAG = "badge_count"
DOT_TAG = "badge_dot"
LABEL_TAG = "badge_count_label"
ANCHOR_TAGS = ["anchor_inbox", "anchor_sync"]


def rgb(fill) -> tuple[int, int, int]:
    return (fill["r"], fill["g"], fill["b"])


def collect_text(node, out: list[str]) -> None:
    if node.get("type") == "Text":
        out.append(node.get("content", ""))
    for child in node.get("children") or []:
        collect_text(child, out)


def texts_of(snap) -> list[str]:
    out: list[str] = []
    collect_text(snap, out)
    return out


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: a count pill + a dot, both anchors present ─────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        count_pill = find_by_tag(snap, COUNT_TAG)
        dot = find_by_tag(snap, DOT_TAG)
        assert count_pill is not None, "count badge pill painted at boot"
        assert dot is not None, "dot badge painted at boot"
        for tag in ANCHOR_TAGS:
            assert find_by_tag(snap, tag) is not None, f"anchor surface {tag} present"
        assert "3" in texts_of(snap), "count label '3' painted at boot"

        # The error-tier red is the badge's distinctive colour — the count
        # pill and the dot share it, and it is distinct from the anchor's
        # SurfaceContainerHighest tone. Source of truth = the paint fills.
        error_rgb = rgb(count_pill["style"]["fill"])
        window_rgb = rgb(snap["style"]["fill"])
        assert_eq(rgb(dot["style"]["fill"]), error_rgb, "dot shares the error-tier fill")
        anchor_rgb = rgb(find_by_tag(snap, ANCHOR_TAGS[0])["style"]["fill"])
        assert error_rgb != anchor_rgb, "the badge red is distinct from the anchor tone"
        # The dot is the small circle variant (radius = half its size); the
        # count pill is a fully-rounded stadium.
        assert_eq(dot["style"]["corner_radius"], 5, "dot is a 10px circle (radius 5)")
        assert_eq(count_pill["style"]["corner_radius"], 9, "count pill is a stadium (radius 9)")

        rects = abs_rects_of(snap)
        for tag in (COUNT_TAG, DOT_TAG, *ANCHOR_TAGS):
            assert tag in rects, f"{tag} has an absolute rect"
        # The count badge sits flush against the inbox anchor's right edge.
        ax, ay, aw, _ = rects[ANCHOR_TAGS[0]]
        bx, by, bw, _ = rects[COUNT_TAG]
        assert_eq(bx + bw, ax + aw, "count badge is flush with the anchor right edge")
        assert by <= ay + 4, "count badge sits at the anchor top"

        # ── count badge: introspect every axis ───────────────────────────
        assert_eq(tf.query(f"/{COUNT_TAG}/external/count"), 3, "boot count is 3")
        assert_eq(tf.query(f"/{COUNT_TAG}/external/max"), 99, "default overflow cap is 99")
        assert_eq(tf.query(f"/{COUNT_TAG}/external/dot"), False, "the count badge is not a dot")
        assert_eq(tf.query(f"/{COUNT_TAG}/external/label"), "3", "label echoes the count")
        assert_eq(tf.query(f"/{COUNT_TAG}/external/visible"), True, "a non-zero count is visible")

        # ── dot badge: introspect every axis ─────────────────────────────
        assert_eq(tf.query(f"/{DOT_TAG}/external/dot"), True, "the dot badge boots as a dot")
        assert_eq(tf.query(f"/{DOT_TAG}/external/count"), 0, "the dot badge carries no count")
        assert_eq(tf.query(f"/{DOT_TAG}/external/label"), "", "a dot badge shows no number")
        assert_eq(tf.query(f"/{DOT_TAG}/external/visible"), True, "a dot badge is shown")

        # ── overflow: label caps at '99+', raw count stays uncapped ───────
        tf.intervene(f"/{COUNT_TAG}/external/count", 150)
        assert_eq(tf.query(f"/{COUNT_TAG}/external/count"), 150, "raw count is uncapped")
        assert_eq(tf.query(f"/{COUNT_TAG}/external/label"), "99+", "label caps at the overflow")
        over = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert "99+" in texts_of(over), "the overflow label '99+' is painted"
        assert find_by_tag(over, COUNT_TAG) is not None, "the overflow pill is still painted"

        # ── count 0 hides the count badge (no pill in the paint scene) ────
        tf.intervene(f"/{COUNT_TAG}/external/count", 0)
        assert_eq(tf.query(f"/{COUNT_TAG}/external/visible"), False, "count 0 is hidden")
        hidden = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(hidden, COUNT_TAG) is None, "a hidden count badge paints no pill"
        assert find_by_tag(hidden, ANCHOR_TAGS[0]) is not None, "the anchor surface still paints"

        # ── restoring a count brings the pill back ────────────────────────
        tf.intervene(f"/{COUNT_TAG}/external/count", 7)
        assert_eq(tf.query(f"/{COUNT_TAG}/external/label"), "7", "restored count shows '7'")
        back = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert "7" in texts_of(back), "the restored '7' pill is painted"

        # ── a custom max moves the overflow point ─────────────────────────
        tf.intervene(f"/{COUNT_TAG}/external/max", 9)
        assert_eq(tf.query(f"/{COUNT_TAG}/external/label"), "7", "7 <= max 9 shows the number")
        tf.intervene(f"/{COUNT_TAG}/external/count", 50)
        assert_eq(tf.query(f"/{COUNT_TAG}/external/label"), "9+", "50 > max 9 overflows to '9+'")
        assert_eq(tf.query(f"/{COUNT_TAG}/external/max"), 9, "the custom cap round-trips")

        # ── the dot badge re-purposes to a count pill ─────────────────────
        tf.intervene(f"/{DOT_TAG}/external/dot", False)
        assert_eq(tf.query(f"/{DOT_TAG}/external/visible"), False,
                  "clearing dot with no count hides it")
        tf.intervene(f"/{DOT_TAG}/external/count", 5)
        assert_eq(tf.query(f"/{DOT_TAG}/external/label"), "5", "the holder now shows a count")
        repurposed = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert "5" in texts_of(repurposed), "the re-purposed count pill paints '5'"

    # ── Phase 2 — live-pixel (boot frame: a '3' count pill + a dot) ──────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    cx, cy, cw, ch = rects[COUNT_TAG]
    dx, dy, dw, dh = rects[DOT_TAG]
    # The pill carries the centred white digit, so sample a few px in from
    # the left edge (inside the rounded circle, clear of the glyph).
    count_fill = (cx + 3, cy + ch // 2)
    dot_centre = (dx + dw // 2, dy + dh // 2)
    window_corner = (5, 5)
    p_count, p_dot, p_window = sample_png_points(
        png, [count_fill, dot_centre, window_corner]
    )
    assert_pixel_eq(p_count, (*error_rgb, 255),
                    f"count pill interior is the error tier {count_fill}", tolerance=12)
    assert_pixel_eq(p_dot, (*error_rgb, 255),
                    f"dot interior is the error tier {dot_centre}", tolerance=12)
    assert_pixel_eq(p_window, (*window_rgb, 255),
                    f"window background is Surface {window_corner}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r759-")) / "badge.png"
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
    sys.exit(run_demo("R759 Badge", body))
