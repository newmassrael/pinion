#!/usr/bin/env python3
"""R757 §5.38 §5.40 Material 3 cards (`hello-card`).

Three M3 card *variants* shown side by side as clickable surfaces:

  * **elevated** — a `Surface`-toned fill that casts an elevation shadow
    (resting Level 1, bumping to Level 2 on hover);
  * **filled** — a higher-tone `SurfaceContainerHighest` fill, flat;
  * **outlined** — a flat `Surface` fill ringed by a 1 px `Outline`
    border.

A clickable card's pointer interaction (`enter → Hover`, `down →
Pressed`, `up → click → Hover`, `leave → Idle`) is byte-identical to a
button's, so each card reuses a `ButtonExternal` (no second, identical
SCXML). The three externals are composed through the R55.D.5
`create_extra_externals` slot (the `hello-accordion` mould), each surface
tagged `card_{variant}`, so the input router routes a click on card *i*
straight to that card's handle. The card surface paint is built inline
(1st card-paint consumer, the R753/R756 inline-first precedent): it
composes the shared elevation ramp (R711) + state-layer overlay (R752),
so the only new code is the variant → (tone, shadow, border) choice.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot: three cards, the variant surfaces distinguished by *data*
    (filled tone != Surface; only outlined carries a border) — the
    surface variant is observable without pixels;
  * AI-first feedback proof: `scene/hover` on a card drives ONLY that
    card's posture to `Hover` (introspected via `/card_X/external/state`),
    and `pointer_leave` rolls it back — real hover feedback, no pixels;
  * the full click arc — `send PointerEnter/Down/Up` walks
    `Idle → Hover → Pressed → Hover` (the `Pressed → Hover` edge is the
    button click; the emitted `click` intent is unit-tested in
    `pinion_widget_paint`/`button`), and a native `scene/click` leaves
    the card `Hover` (cursor still over) then `Idle` after leave;
  * keyboard: each card is its own Tab stop (`focus/set`), and
    `ArrowRight` / `ArrowLeft` / `Home` / `End` rove focus between the
    cards (wrapping), observed via `focus/get`.

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame; source of truth = the
paint scene's own surface fills, so the assertion is introspection<->screen
parity, not a hardcoded palette guess):
  * the window background is Surface;
  * the filled card's interior is the higher SurfaceContainerHighest tone
    (so it is pixel-distinguishable from the Surface background that shows
    through the flat elevated / outlined cards).

Run from the workspace root:
    cargo build -p hello-card --release
    python3 tools/demos/r757_card.py
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

EXAMPLE = "hello-card"
VIEWPORT = (560, 220)
TAGS = ["card_elevated", "card_filled", "card_outlined"]
TITLES = ["Elevated", "Filled", "Outlined"]


def state(tf, tag: str):
    return tf.query(f"/{tag}/external/state")


def send(tf, tag: str, event: str):
    return tf.invoke(f"/{tag}/external/send", event)


def focused(tf):
    return tf.request("focus/get", {}).result.get("focused")


def rove(tf, name: str) -> str:
    """Press a roving key while a card holds focus, return the new focus."""
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
        # ── boot: three variant surfaces ─────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        for tag in TAGS:
            assert find_by_tag(snap, tag) is not None, f"card surface {tag} present"
        texts: list[str] = []
        collect_text(snap, texts)
        for title in TITLES:
            assert title in texts, f"headline {title} painted"

        # Surface variants are distinguishable by *data*, not pixels.
        elevated = find_by_tag(snap, TAGS[0])["style"]
        filled = find_by_tag(snap, TAGS[1])["style"]
        outlined = find_by_tag(snap, TAGS[2])["style"]
        surface_rgb = rgb(snap["style"]["fill"])
        assert_eq(rgb(elevated["fill"]), surface_rgb, "elevated card rests on the Surface tone")
        assert_eq(rgb(outlined["fill"]), surface_rgb, "outlined card rests on the Surface tone")
        filled_rgb = rgb(filled["fill"])
        assert filled_rgb != surface_rgb, "filled card sits on a distinct higher tone"
        assert outlined["border"] is not None, "only the outlined card carries a border"
        assert elevated["border"] is None, "elevated card has no border"
        assert filled["border"] is None, "filled card has no border"

        # Elevation is the elevated card's distinctive feature — observable
        # as the (key + ambient) shadow list in the scene data, flat for
        # the other two variants.
        assert len(elevated["shadows"]) == 2, "elevated card casts a key+ambient shadow"
        assert filled["shadows"] == [], "filled card is flat (no shadow)"
        assert outlined["shadows"] == [], "outlined card is flat (no shadow)"

        rects = abs_rects_of(snap)
        for tag in TAGS:
            assert tag in rects, f"{tag} has an absolute rect"

        # ── AI-first feedback proof: hover drives ONLY that card ─────────
        for i, tag in enumerate(TAGS):
            assert_eq(state(tf, tag), "Idle", f"{tag} idle before hover")
            tf.hover(path=tag)
            assert_eq(state(tf, tag), "Hover", f"hover drives {tag} to Hover (real feedback)")
            for other in TAGS:
                if other != tag:
                    assert_eq(state(tf, other), "Idle", f"sibling {other} untouched by hover on {tag}")
            tf.pointer_leave()
            assert_eq(state(tf, tag), "Idle", f"pointer_leave rolls {tag} back to Idle")

        # ── hovering the elevated card bumps its elevation (L1 -> L2) ────
        rest_blur = elevated["shadows"][0]["blur"]
        tf.hover(path=TAGS[0])
        hover_snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        hover_blur = find_by_tag(hover_snap, TAGS[0])["style"]["shadows"][0]["blur"]
        assert hover_blur > rest_blur, \
            f"hover bumps the elevated card's shadow ({rest_blur} -> {hover_blur})"
        tf.pointer_leave()
        assert_eq(state(tf, TAGS[0]), "Idle", "elevated card back to Idle after the bump check")

        # ── full click arc via the typed send channel ────────────────────
        arc = TAGS[2]
        assert_eq(send(tf, arc, "PointerEnter"), "Hover", "PointerEnter -> Hover")
        assert_eq(send(tf, arc, "PointerDown"), "Pressed", "PointerDown -> Pressed")
        assert_eq(send(tf, arc, "PointerUp"), "Hover", "PointerUp -> Hover (click edge)")
        assert_eq(send(tf, arc, "PointerLeave"), "Idle", "PointerLeave -> Idle")

        # ── native click lands on the card (cursor stays over -> Hover) ──
        tf.click(path=TAGS[0])
        assert_eq(state(tf, TAGS[0]), "Hover", "native click leaves the card Hover (cursor over)")
        assert_eq(state(tf, TAGS[1]), "Idle", "the click did not touch a sibling card")
        tf.pointer_leave()
        assert_eq(state(tf, TAGS[0]), "Idle", "pointer_leave after click rolls back to Idle")

        # ── keyboard: each card is its own Tab stop + roving focus ───────
        for tag in TAGS:
            assert_eq(
                tf.request("focus/set", {"tag": tag}).result.get("focused"),
                tag,
                f"{tag} is an independent Tab stop",
            )
        tf.request("focus/set", {"tag": TAGS[0]})
        assert_eq(rove(tf, "ArrowRight"), TAGS[1], "ArrowRight roves to the next card")
        assert_eq(rove(tf, "ArrowRight"), TAGS[2], "ArrowRight again roves to the third card")
        assert_eq(rove(tf, "ArrowRight"), TAGS[0], "ArrowRight wraps past the last card")
        assert_eq(rove(tf, "ArrowLeft"), TAGS[2], "ArrowLeft wraps before the first card")
        assert_eq(rove(tf, "Home"), TAGS[0], "Home jumps to the first card")
        assert_eq(rove(tf, "End"), TAGS[2], "End jumps to the last card")

    # ── Phase 2 — live-pixel (boot frame: three idle cards) ──────────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    fx, fy, fw, fh = rects[TAGS[1]]   # filled card rect
    # Sample horizontal-centre near the bottom edge: below both left-
    # aligned text lines and clear of the 12 px rounded corners, so the
    # read hits the solid SurfaceContainerHighest fill.
    filled_interior = (fx + fw // 2, fy + fh - 18)
    window_corner = (5, 5)
    p_filled, p_window = sample_png_points(png, [filled_interior, window_corner])
    assert_pixel_eq(p_filled, (*filled_rgb, 255),
                    f"filled card interior is SurfaceContainerHighest {filled_interior}", tolerance=10)
    assert_pixel_eq(p_window, (*surface_rgb, 255),
                    f"window background is Surface {window_corner}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r757-")) / "card.png"
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
    sys.exit(run_demo("R757 Material 3 cards", body))
