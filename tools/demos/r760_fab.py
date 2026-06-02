#!/usr/bin/env python3
"""R760 §5.38 §5.40 Floating Action Button (`hello-fab`).

The Material 3 FAB in its four size variants shown side by side as
clickable surfaces:

  * **small** (40, radius 12), **standard** (56, radius 16), **large**
    (96, radius 28) — icon-only rounded squares;
  * **extended** (radius 16) — a wider pill carrying an icon + label.

A FAB is an elevated accent button: its pointer interaction is
byte-identical to a button's, so each FAB reuses a `ButtonExternal`
(composed through `create_extra_externals`, the hello-card mould). The
*paint* reuses the M3 button substrate too — `view_button` +
`ButtonColors::accent` — plus the new R760 `ButtonStyle` elevation axis
(additive, default 0). So every FAB rests on the accent container tone,
casts the shared elevation (key + ambient) shadow, and lifts the shadow
on hover (L3 -> L4); there is zero new paint code in the example.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot: four accent surfaces distinguished by *data* — same accent fill,
    M3 corner-radius ramp (12/16/28/16), and a 2-shadow elevated surface
    each — observable without pixels;
  * hover drives ONLY the hovered FAB to `Hover` (introspected via
    `/fab_X/external/state`) and bumps its elevation shadow (L3 -> L4);
  * the full click arc `Idle -> Hover -> Pressed -> Hover -> Idle`;
  * keyboard: each FAB is its own Tab stop, and `ArrowRight` / `Home` /
    `End` rove focus between them (the shared `activate_or_rove` SSOT).

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame; source of truth = the
paint scene's own fills, so the assertion is introspection<->screen
parity):
  * the window background is Surface;
  * every FAB interior is the accent container tone.

Run from the workspace root:
    cargo build -p hello-fab --release
    python3 tools/demos/r760_fab.py
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

EXAMPLE = "hello-fab"
VIEWPORT = (520, 260)
TAGS = ["fab_small", "fab_standard", "fab_large", "fab_extended"]
CAPTIONS = ["Small", "Standard", "Large", "Extended"]
RADII = [12, 16, 28, 16]


def state(tf, tag: str):
    return tf.query(f"/{tag}/external/state")


def send(tf, tag: str, event: str):
    return tf.invoke(f"/{tag}/external/send", event)


def focused(tf):
    return tf.request("focus/get", {}).result.get("focused")


def rove(tf, name: str) -> str:
    tf.key(path=focused(tf), name=name)
    return focused(tf)


def collect_text(node, out: list[str]) -> None:
    if node.get("type") == "Text":
        out.append(node.get("content", ""))
    for child in node.get("children") or []:
        collect_text(child, out)


def rgb(fill) -> tuple[int, int, int]:
    return (fill["r"], fill["g"], fill["b"])


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: four accent, elevated, size-ramped surfaces ────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        for tag in TAGS:
            assert find_by_tag(snap, tag) is not None, f"FAB surface {tag} present"
        texts = []
        collect_text(snap, texts)
        for caption in CAPTIONS:
            assert caption in texts, f"caption {caption} painted"

        accent_rgb = rgb(find_by_tag(snap, TAGS[0])["style"]["fill"])
        window_rgb = rgb(snap["style"]["fill"])
        assert accent_rgb != window_rgb, "the accent FAB tone is distinct from the Surface window"
        for tag, radius in zip(TAGS, RADII):
            style = find_by_tag(snap, tag)["style"]
            assert_eq(rgb(style["fill"]), accent_rgb, f"{tag} rests on the accent container tone")
            assert_eq(style["corner_radius"], radius, f"{tag} carries its M3 corner radius")
            assert len(style["shadows"]) == 2, f"{tag} casts a 2-part elevation shadow (elevated)"

        rects = abs_rects_of(snap)
        for tag in TAGS:
            assert tag in rects, f"{tag} has an absolute rect"

        # ── AI-first feedback: hover drives ONLY that FAB + lifts it ─────
        for tag in TAGS:
            assert_eq(state(tf, tag), "Idle", f"{tag} idle before hover")
            tf.hover(path=tag)
            assert_eq(state(tf, tag), "Hover", f"hover drives {tag} to Hover (real feedback)")
            for other in TAGS:
                if other != tag:
                    assert_eq(state(tf, other), "Idle", f"sibling {other} untouched by hover on {tag}")
            tf.pointer_leave()
            assert_eq(state(tf, tag), "Idle", f"pointer_leave rolls {tag} back to Idle")

        # hovering lifts the elevation (L3 -> L4 -> larger blur)
        rest_blur = find_by_tag(snap, TAGS[2])["style"]["shadows"][0]["blur"]
        tf.hover(path=TAGS[2])
        hover_snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        hover_blur = find_by_tag(hover_snap, TAGS[2])["style"]["shadows"][0]["blur"]
        assert hover_blur > rest_blur, f"hover lifts the FAB elevation ({rest_blur} -> {hover_blur})"
        tf.pointer_leave()
        assert_eq(state(tf, TAGS[2]), "Idle", "back to Idle after the lift check")

        # ── full click arc via the typed send channel ────────────────────
        arc = TAGS[3]
        assert_eq(send(tf, arc, "PointerEnter"), "Hover", "PointerEnter -> Hover")
        assert_eq(send(tf, arc, "PointerDown"), "Pressed", "PointerDown -> Pressed")
        assert_eq(send(tf, arc, "PointerUp"), "Hover", "PointerUp -> Hover (click edge)")
        assert_eq(send(tf, arc, "PointerLeave"), "Idle", "PointerLeave -> Idle")

        # ── native click lands on the FAB (cursor stays over -> Hover) ───
        tf.click(path=TAGS[0])
        assert_eq(state(tf, TAGS[0]), "Hover", "native click leaves the FAB Hover (cursor over)")
        assert_eq(state(tf, TAGS[1]), "Idle", "the click did not touch a sibling FAB")
        tf.pointer_leave()
        assert_eq(state(tf, TAGS[0]), "Idle", "pointer_leave after click rolls back to Idle")

        # ── keyboard: each FAB its own Tab stop + roving focus ───────────
        for tag in TAGS:
            assert_eq(
                tf.request("focus/set", {"tag": tag}).result.get("focused"),
                tag,
                f"{tag} is an independent Tab stop",
            )
        tf.request("focus/set", {"tag": TAGS[0]})
        assert_eq(rove(tf, "ArrowRight"), TAGS[1], "ArrowRight roves to the next FAB")
        assert_eq(rove(tf, "ArrowRight"), TAGS[2], "ArrowRight again roves on")
        assert_eq(rove(tf, "End"), TAGS[3], "End jumps to the last FAB")
        assert_eq(rove(tf, "ArrowRight"), TAGS[0], "ArrowRight wraps past the last FAB")
        assert_eq(rove(tf, "Home"), TAGS[0], "Home stays at the first FAB")
        assert_eq(rove(tf, "ArrowLeft"), TAGS[3], "ArrowLeft wraps before the first FAB")

    # ── Phase 2 — live-pixel (boot frame: four idle accent FABs) ─────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    # Sample 12 px in from each FAB's left edge at mid-height: inside the
    # accent surface and clear of the centred icon / label glyph.
    points = []
    for tag in TAGS:
        x, y, _w, h = rects[tag]
        points.append((x + 12, y + h // 2))
    window_corner = (5, 5)
    samples = sample_png_points(png, [*points, window_corner])
    for tag, px in zip(TAGS, samples[:-1]):
        assert_pixel_eq(px, (*accent_rgb, 255), f"{tag} interior is the accent tone", tolerance=12)
    assert_pixel_eq(samples[-1], (*window_rgb, 255),
                    f"window background is Surface {window_corner}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r760-")) / "fab.png"
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
    sys.exit(run_demo("R760 Floating Action Button", body))
