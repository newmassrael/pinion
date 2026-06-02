#!/usr/bin/env python3
"""R756 §5.38 §5.40 Material 3 input chips (`hello-input-chip`).

A row of five removable preset recipient tokens (Alice / Bob / Carol /
Dave / Erin), each a content-hugging outlined M3 chip with a trailing `×`
delete affordance. This is the **2nd consumer** of the chip paint lift
(`pinion_widget_paint::chip`): the filter chip (R753) built the chip body
inline; an input chip is the same M3 container skin (8 px radius, 36 px
tall, optional outline, state-layer-tinted) differing only in width
strategy (content-hug vs fixed), children (trailing `×` vs leading check),
and fill choice — so R756 lifts the shared core.

The delete machinery mirrors `todomvc`: the chip collection is a
`Signal<Vec<InputChip>>` (seeded with five stable-id tokens) and a single
`ChipDeleteExternal` (primary tag `chip_delete`) owns both the delete and
the per-chip `×` `Button` SCXML. The R51.42 §5.35 composite-tag wire
forwards each `chip_delete#<id>` pointer event as `"<id>:<EventName>"`; the
external drives that chip's button posture (so the `×` shows real M3
hover / pressed state-layer feedback) and the canonical button-like
`Pressed → Hover` click edge commits the delete. A removed chip drops out
of the `Vec` and is no longer painted — the row shrinks.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot: five chips, ids 1..=5; the group + every `chip_delete#<id>`
    sub-target paint, each chip carries its label + a `×` glyph;
  * AI-first feedback proof: `scene/hover` on a `×` drives ONLY that
    chip's button posture to `Hover` (introspected via `state:<id>`), and
    `pointer_leave` rolls it back — the real hover feedback, observed
    without pixels;
  * `scene/click` on a `×` removes that chip (composite-tag wire); the
    survivors keep their original ids/tags (no resequencing);
  * `scene/invoke {/external/delete}` is the typed-id AI-driving path and
    returns `was_present`;
  * keyboard: each `×` is its own Tab stop; `Enter` on a focused `×`
    removes that chip;
  * a stale `×` tag (already-deleted chip) is a safe no-op — the count
    does not change.

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame; source of truth = the
paint scene's own surface fill, so the assertion is introspection<->screen
parity, not a hardcoded palette guess):
  * the window background is Surface;
  * an idle input chip is transparent (outlined, no fill), so the window
    Surface shows through its `×` slot.

Run from the workspace root:
    cargo build -p hello-input-chip --release
    python3 tools/demos/r756_input_chip.py
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

EXAMPLE = "hello-input-chip"
VIEWPORT = (520, 120)
GROUP = "chip_delete"
N = 5
LABELS = ["Alice", "Bob", "Carol", "Dave", "Erin"]
CLOSE_GLYPH = "✕"  # U+2715 MULTIPLICATION X — matches main.rs CLOSE_GLYPH


def close_tag(chip_id: int) -> str:
    return f"chip_delete#{chip_id}"


def count(tf) -> int:
    return tf.query("/external/count")


def ids(tf) -> list[int]:
    return list(tf.query("/external/ids"))


def posture(tf, chip_id: int):
    """The `×` button posture variant name for chip `chip_id`."""
    return tf.query(f"/external/state:{chip_id}")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def collect_text(node, out: list[str]) -> None:
    if node.get("type") == "Text":
        out.append(node.get("content", ""))
    for child in node.get("children") or []:
        collect_text(child, out)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: five stable-id tokens ──────────────────────────────────
        assert_eq(count(tf), 5, "boot: five seeded chips")
        assert_eq(ids(tf), [1, 2, 3, 4, 5], "boot ids are the stable 1..=5")

        snap = paint(tf)
        assert find_by_tag(snap, GROUP) is not None, "group row tagged chip_delete"
        for cid in range(1, N + 1):
            assert find_by_tag(snap, close_tag(cid)) is not None, \
                f"chip {cid} × sub-target present"
        texts: list[str] = []
        collect_text(snap, texts)
        for label in LABELS:
            assert label in texts, f"label {label} painted"
        assert_eq(texts.count(CLOSE_GLYPH), N, "one × glyph per chip")

        # Capture boot geometry + surface tone for the Phase 2 pixel guard
        # (the screenshot is a fresh boot, so these match it).
        rects = abs_rects_of(snap)
        assert close_tag(1) in rects, "chip 1 × has an absolute rect"
        surface_fill = snap["style"]["fill"]
        surface_rgb = (surface_fill["r"], surface_fill["g"], surface_fill["b"])

        # ── AI-first feedback proof: hover drives ONLY that × ────────────
        assert_eq(posture(tf, 2), "Idle", "× 2 idle before hover")
        tf.hover(path=close_tag(2))
        assert_eq(posture(tf, 2), "Hover", "hover drives × 2 to Hover (real feedback)")
        assert_eq(posture(tf, 1), "Idle", "sibling × 1 untouched by the hover")
        tf.pointer_leave()
        assert_eq(posture(tf, 2), "Idle", "pointer_leave rolls × 2 back to Idle")

        # ── delete the middle chip (Carol #3) via composite-tag click ────
        tf.click(path=close_tag(3))
        tf.pointer_leave()
        assert_eq(count(tf), 4, "click × on Carol removes one chip")
        assert_eq(ids(tf), [1, 2, 4, 5], "Carol (#3) gone; survivors keep their ids")
        snap = paint(tf)
        assert find_by_tag(snap, close_tag(3)) is None, "deleted chip not painted"
        for cid in (1, 2, 4, 5):
            assert find_by_tag(snap, close_tag(cid)) is not None, \
                f"survivor {cid} keeps its original tag (no resequencing)"

        # ── typed-id delete (AI-driving path) returns was_present ────────
        assert_eq(tf.invoke("/external/delete", 4), True, "delete #4 reports was_present")
        assert_eq(count(tf), 3, "typed delete shrinks the collection")
        assert_eq(ids(tf), [1, 2, 5], "Dave (#4) gone")
        assert_eq(tf.invoke("/external/delete", 4), False, "re-deleting #4 reports absent")
        assert_eq(count(tf), 3, "stale typed delete is a no-op")

        # ── keyboard: Enter on a focused × removes that chip ─────────────
        focused = tf.request("focus/set", {"tag": close_tag(5)}).result.get("focused")
        assert_eq(focused, close_tag(5), "chip 5 × is an independent Tab stop")
        tf.key(path=close_tag(5), name="Enter")
        assert_eq(count(tf), 2, "Enter on focused × removes Erin (#5)")
        assert_eq(ids(tf), [1, 2], "only Alice + Bob remain")
        snap = paint(tf)
        assert find_by_tag(snap, close_tag(5)) is None, "keyboard-deleted chip not painted"

        # ── stale × tag (already-deleted) is a safe no-op ────────────────
        try:
            tf.click(path=close_tag(3))  # Carol was deleted above
        except Exception:  # noqa: BLE001 — RpcError when the tag is absent
            pass
        assert_eq(count(tf), 2, "clicking a deleted chip's × changes nothing")
        assert_eq(ids(tf), [1, 2], "collection unchanged by the stale click")

    # ── Phase 2 — live-pixel (boot frame: five idle outlined chips) ──────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    cx, cy, cw, ch = rects[close_tag(1)]   # chip 1's × slot (transparent at idle)
    # Sample a corner of the × slot, clear of the centred glyph, so the
    # transparent state layer lets the window Surface show through.
    chip_corner = (cx + 2, cy + 2)
    window_corner = (5, 5)
    p_chip, p_window = sample_png_points(png, [chip_corner, window_corner])
    assert_pixel_eq(p_chip, (*surface_rgb, 255),
                    f"idle chip × slot shows Surface (transparent) {chip_corner}", tolerance=12)
    assert_pixel_eq(p_window, (*surface_rgb, 255),
                    f"window background is Surface {window_corner}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r756-")) / "input-chip.png"
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
    sys.exit(run_demo("R756 Material 3 input chips", body))
