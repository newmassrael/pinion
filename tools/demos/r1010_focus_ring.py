#!/usr/bin/env python3
"""R1010 §5.39 §5.40 — content-surface focus-ring opt-out E2E.

Drives `hello-grid-pointer` (the focusable terminal pane) via JSON-RPC. R1009
made the pane focusable for key input, which drew a framework focus ring around
the whole viewport — but a terminal owns its focus indicator (its text cursor),
so the ring is wrong. R1010 adds `WidgetView::focus_ring_style(focused_tag) ->
Option<FocusRingStyle>`; the pane returns `None` to suppress the ring while
still taking focus for `apply_key`.

The ring is a `pinion_overlay` box tagged `ai-overlay/focus-ring`, injected by
the shell's produce path, so the snapshot shows it when present.

  (A) the pane suppresses its ring — focus the pane, the ring stays ABSENT,
      and the pane still works focused (apply_key records keys, the pointer
      still resolves cells).
  (B) the contrast — a default-ring control (`hello-cell-select`) DOES draw the
      ring on focus through the same focus/set mechanism, so (A)'s absence is a
      genuine opt-out, not a missing-focus artefact.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the terminal grid paints (no ring).
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
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-grid-pointer"
WIN = (640, 384)
ROOT_TAG = "grid_pointer"
RING = "ai-overlay/focus-ring"
AT = (320.0, 192.0)

# Control: an existing default-ring focusable widget (R953 cell-select grid).
CONTROL = "hello-cell-select"
CONTROL_TAG = "grid"
CONTROL_WIN = (400, 480)


def q(d, path: str):
    return d.query(f"/external/{path}")


def ring_present(d, win) -> bool:
    return RING in abs_rects_of(d.snapshot(source="paint", viewport=win))


def body() -> None:
    # ── (A) the terminal pane suppresses its focus ring ────────────
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        rects = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
        assert ROOT_TAG in rects, "terminal pane present"
        assert RING not in rects, "no ring before focus"

        # Focus the pane — it takes focus for apply_key, but draws NO ring.
        tf.request("focus/set", {"tag": ROOT_TAG})
        assert not ring_present(tf, WIN), "focused pane suppresses the framework ring"

        # The pane is genuinely focused: apply_key records keys (R1009), and the
        # ring stays absent through every key — focus without a ring.
        for name in ("Backspace", "Delete", "Insert", "F1", "F5", "F12", "ArrowDown", "Home", "Enter"):
            tf.key(at=AT, name=name)
            assert_eq(q(tf, "last_key"), name, f"focused pane handles {name}")
            assert not ring_present(tf, WIN), f"still no ring after {name}"

        # The pointer hit-test (R1008) still resolves while focused + ring-free.
        tf.click(at=(200.0, 100.0))
        assert_eq(q(tf, "cell"), "25,6", "pointer-to-cell works on the focused pane")
        assert not ring_present(tf, WIN), "still no ring after a click"

    # ── (B) contrast: a default-ring control DOES ring on focus ────
    with RpcSubprocess(CONTROL, boot_grace=1.5) as cf:
        crects = abs_rects_of(cf.snapshot(source="paint", viewport=CONTROL_WIN))
        assert CONTROL_TAG in crects, "control grid present"
        assert RING not in crects, "control: no ring before focus"
        cf.request("focus/set", {"tag": CONTROL_TAG})
        cring = abs_rects_of(cf.snapshot(source="paint", viewport=CONTROL_WIN))
        assert RING in cring, "control DRAWS the default ring on focus (the opt-out is real)"

    # ── Phase 2 — live pixels (the pane paints, no ring) ───────────
    snap, rects = _boot_snapshot_and_rects()
    assert ROOT_TAG in rects and RING not in rects, "pane paints, no ring at boot"
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, f"screenshot {img.width}x{img.height} != {WIN}"
    gx, gy, gw, gh = rects[ROOT_TAG]
    p0, p1 = sample_png_points(img, [(gx + 4, gy + 8), (gx + gw // 2, gy + gh // 2)])
    assert p0 is not None and p1 is not None, "grid cells sampled"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r1010-")) / "ring.png"
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
    sys.exit(run_demo("R1010 §5.39 §5.40 — content-surface focus-ring opt-out", body))
