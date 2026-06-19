#!/usr/bin/env python3
"""R1009 §5.13 — content-surface named-key vocabulary E2E.

Drives `hello-grid-pointer` (now a focusable terminal pane) via JSON-RPC. A
real terminal forwards EVERY key to its child; before R1009 the shell's
`named_key_str` bridge surfaced only ~10 keys and dropped Backspace / Delete /
Insert / F1-F12 on the winit path, so an interactive terminal could not receive
them (sprag-gui R27's forcing case). R1009 widens the curated bridge to the
content-surface editing + function vocabulary (device/media keys stay None).

This demo focuses the pane and confirms each key reaches `apply_key` (recorded
as `last_key`). Note the RPC `scene/key` plane routes a named string straight to
`handle_named_key` (it always bypassed `named_key_str`), so the WINIT-path fix
is pinned by `pinion-shell` unit tests (`r1009_named_key_str_tests`); this demo
verifies the consumer half — the pane handles the full vocabulary apply_key now
receives.

  * primary `GridPointerExternal` at `/external/` — `last_key` is the W3C
    `KeyboardEvent.key` string the binding's `apply_key` last forwarded (the
    byte a sprag pane would encode for its PTY).

  (A) editing keys — Backspace / Delete / Insert reach apply_key.
  (B) function row — F1-F12 reach apply_key.
  (C) navigation / activation baseline — arrows / Home / End / Page / Enter
      still reach apply_key (byte-unchanged for existing widgets).

  Phase 2 — PIXELS (PINION_SCREENSHOT): the terminal grid paints.
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
AT = (320.0, 192.0)


def q(d, path: str):
    return d.query(f"/external/{path}")


def send(d, name: str) -> None:
    d.key(at=AT, name=name)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        rects = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
        assert ROOT_TAG in rects, "focusable terminal pane present"
        assert_eq(q(tf, "last_key"), "none", "no key before any input")

        # Focus the pane so the shell routes keys to its apply_key.
        tf.request("focus/set", {"tag": ROOT_TAG})

        # ── (A) editing keys — the R1009 forcing case ───────────────
        for name in ("Backspace", "Delete", "Insert"):
            send(tf, name)
            assert_eq(q(tf, "last_key"), name, f"{name} reaches apply_key")

        # ── (B) function row F1-F12 ─────────────────────────────────
        for i in range(1, 13):
            name = f"F{i}"
            send(tf, name)
            assert_eq(q(tf, "last_key"), name, f"{name} reaches apply_key")

        # ── (C) navigation / activation baseline unchanged ──────────
        for name in (
            "ArrowUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Enter",
        ):
            send(tf, name)
            assert_eq(q(tf, "last_key"), name, f"{name} still reaches apply_key")

    # ── Phase 2 — live pixels (the terminal grid paints) ───────────
    snap, rects = _boot_snapshot_and_rects()
    assert ROOT_TAG in rects, "grid painted at boot"
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
    out = Path(tempfile.mkdtemp(prefix="pinion-r1009-")) / "keys.png"
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
    sys.exit(run_demo("R1009 §5.13 — content-surface named-key vocabulary", body))
