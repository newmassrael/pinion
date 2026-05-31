#!/usr/bin/env python3
"""R725 §5.28 §5.38 §5.40 Snackbar/Toast — timed-auto-dismiss + inverse tier.

A Show button raises a Material 3 snackbar: an `inverseSurface` bar with an
`inverseOnSurface` message and an `inversePrimary` UNDO action — the first
`ColorRole::InversePrimary` consumer (clears the R723 carry). The snackbar
auto-dismisses after 4 s via the §5.28 `SnackbarTimer` Tickable, and because the
dismissal rides the animation driver it is **deterministically drivable** by the
R724 `scene/tick` RPC.

Verification (structure-only, per [[ai-first-rpc-introspection-obligation]]):
the snackbar is an RPC-triggered *overlay*, so PINION_SCREENSHOT (boot frame)
cannot capture it ([[introspection-from-paint-not-screen]] / R711 precedent);
its inverse tones are verified as DATA via scene/snapshot (the R723 hello-theme
`inverse_swatch` already provides the live-pixel proof that inverseSurface
rasterizes). The killer assertion is the scene/tick auto-dismiss:

  - boot: snackbar absent;
  - click Show: snackbar present, inverseSurface fill, inverseOnSurface message,
    inversePrimary UNDO label (all three inverse-tier roles);
  - scene/tick(0.5): still present (4 s horizon);
  - scene/tick(4.0): auto-dismissed (deterministic, no wall-clock wait);
  - Show then UNDO: dismissed early + undo counter bumped.

Run from the workspace root:
    cargo build -p hello-snackbar --release
    python3 tools/demos/r725_snackbar.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (360, 300)

# Mirror the light-palette inverse `const`s in crates/pinion-core/src/theme.rs.
INVERSE_SURFACE = (0x32, 0x2F, 0x35)
INVERSE_ON_SURFACE = (0xF5, 0xEF, 0xF7)
INVERSE_PRIMARY = (0x9E, 0xCA, 0xFF)


def rgb(c: dict) -> tuple[int, int, int]:
    return (c["r"], c["g"], c["b"])


def find_text(node, content: str):
    """Depth-first search for a Text node whose content matches."""
    if not isinstance(node, dict):
        return None
    if node.get("type") == "Text" and node.get("content") == content:
        return node
    for child in node.get("children", []) or []:
        hit = find_text(child, content)
        if hit is not None:
            return hit
    # Some snapshot nodes nest content under "content" (Scroll) — ignore.
    return None


def snack_present(snap) -> bool:
    return find_by_tag(snap, "snackbar") is not None


def body() -> None:
    with RpcSubprocess("hello-snackbar") as d:
        # ── boot: snackbar hidden ────────────────────────────────────
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert not snack_present(snap), "snackbar hidden at boot"

        # ── Show: snackbar appears with all three inverse-tier roles ─
        d.click(path="show_snack")
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        snack = find_by_tag(snap, "snackbar")
        assert snack is not None, "snackbar visible after Show"
        assert_eq(rgb(snack["style"]["fill"]), INVERSE_SURFACE, "snackbar fill = inverseSurface")

        msg = find_text(snack, "Message sent")
        assert msg is not None, "message text present"
        assert_eq(rgb(msg["style"]["fg_color"]), INVERSE_ON_SURFACE,
                  "message fg = inverseOnSurface")

        undo = find_text(snack, "UNDO")
        assert undo is not None, "UNDO action label present"
        assert_eq(rgb(undo["style"]["fg_color"]), INVERSE_PRIMARY,
                  "UNDO label fg = inversePrimary (first inversePrimary consumer)")

        # ── scene/tick deterministic auto-dismiss ────────────────────
        d.tick(0.5)
        assert snack_present(d.snapshot(source="paint", viewport=VIEWPORT)), \
            "still visible 0.5 s into the 4 s countdown"
        d.tick(4.0)  # cross the 4 s horizon
        assert not snack_present(d.snapshot(source="paint", viewport=VIEWPORT)), \
            "auto-dismissed after scene/tick past 4 s"

        # ── Show again, then UNDO dismisses early + bumps counter ────
        d.click(path="show_snack")
        assert snack_present(d.snapshot(source="paint", viewport=VIEWPORT)), "re-shown"
        d.click(path="snack_undo")
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert not snack_present(snap), "UNDO dismissed the snackbar"
        assert find_text(snap, "Undone 1 time(s)") is not None, "undo counter bumped to 1"

        # ── a second show/UNDO bumps the counter again ───────────────
        d.click(path="show_snack")
        d.click(path="snack_undo")
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert find_text(snap, "Undone 2 time(s)") is not None, "undo counter bumped to 2"


if __name__ == "__main__":
    sys.exit(run_demo("R725 Snackbar timed-auto-dismiss", body))
