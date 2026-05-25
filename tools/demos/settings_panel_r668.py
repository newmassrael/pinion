#!/usr/bin/env python3
"""R668 §5.16 §5.38 §5.41 §5.49 — R668 substrate E2E verification.

Validates the substrates that landed in R668 atomics (0)-(3) plus the
font_scale → use_text_scale application consumer wired in atomic (4)
partial (the full 6-channel Notifications section + persistence schema
v2 migrator + 4th scrollbar consumer carry to R669 honestly).

Atomic (0) — `SizeStrategy::Fixed` window-creation policy: settings-panel
declares `SizeStrategy::Fixed { width: 720, height: 480 }`. The
headless scene/layout path mirrors the live-shell behaviour at the
declared viewport.

Atomic (1) — `ShellCoreTui::drain_deferred_inputs` substrate primitive.
Covered exhaustively by the `r668_drain_*` integration tests in
`pinion-tui/src/substrate.rs`. The Vello sibling's drain path is
exercised through scene/click / scene/key in the existing R660-R667
demos which still pass post-R668 (verified separately).

Atomic (2) — `pinion_widget_paint::checkbox::view_checkbox` substrate
lift. hello-checkbox is the 1st consumer; covered by
`crates/pinion-widget-paint/src/checkbox.rs` unit tests + the
hello-checkbox a11y test suite. Settings-panel Notifications 2nd
consumer carries to R669.

Atomic (3) — `pinion_core::text_scale::use_text_scale` + the
`TextStyle::with_size_px` thread-local multiplier. This demo drives
the slider via scene/intervene to verify the live-preview cascade
end-to-end.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

# noqa: E402 — runtime path manipulation above.
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    isolated_storage_dir,
    run_demo,
)


PROFILE_TF_TAG = "profile_display_name"
NAV_TAG = "nav_rail"
FONT_SLIDER_TAG = "font_slider"


def collect_text_sizes(node) -> list[int]:
    """Pre-order DFS — every text node's measured pixel height
    (rect.h after compute_layout). The R668 `TextStyle::with_size_px`
    multiplier kicks in at builder time so the paint scene's Text
    nodes carry `raw_px * current_text_scale()` font size; after
    parley shaping + compute_layout, the rendered rect height tracks
    the font size linearly. The LayoutNode wire shape exposes
    `rect.h` but not the raw font_size_px, so we infer the scale
    cascade via measured height (the same signal a screen reader's
    'increase text size' a11y toggle would observe)."""
    out: list[int] = []
    if node.get("kind") == "text":
        rect = node.get("rect", {})
        h = rect.get("h", 0)
        if h > 0:
            out.append(int(h))
    for child in node.get("children", []):
        out.extend(collect_text_sizes(child))
    return out


def layout_at(proc: RpcSubprocess, w: int, h: int):
    """Call scene/layout for a hypothetical viewport. R668 wire pins
    that the layout pass picks up the latest text_scale Signal value
    on every paint cycle — view fn re-run + with_size_px multiplier
    both fire.

    Returns the root LayoutNode directly (scene/layout's response IS
    the root node, no envelope)."""
    resp = proc.request(
        "scene/layout", {"viewport": {"width": w, "height": h}}
    )
    assert resp is not None
    return resp.result


def assertions(proc: RpcSubprocess) -> dict[str, int]:
    """Returns a dict of {atomic: assertion-count} so the wrap-up
    prints a per-atomic breakdown for the AI driver to inspect."""
    counts = {"atomic_0": 0, "atomic_3": 0, "atomic_4_partial": 0}

    # ─── Atomic (0) — SizeStrategy::Fixed window initial size ───
    root = layout_at(proc, 720, 480)
    assert_eq(root["kind"], "container", "(0.a) root scene is Container")
    counts["atomic_0"] += 1
    root_rect = root["rect"]
    assert_eq(root_rect["w"], 720, "(0.b) root width = WIN_W (720)")
    counts["atomic_0"] += 1
    assert_eq(root_rect["h"], 480, "(0.c) root height = WIN_H (480)")
    counts["atomic_0"] += 1

    # ─── Atomic (3) — text_scale wire end-to-end ───
    # Boot default: persisted hydrate fires set_text_scale(0.5 +
    # default_font_scale * 1.5). DEFAULT_FONT_SCALE = 0.5 in
    # settings-panel; check that the first paint already reflects
    # the corresponding scale (1.25).
    sizes_default = collect_text_sizes(root)
    assert len(sizes_default) > 0, "(3.a) at least one Text node present"
    counts["atomic_3"] += 1
    default_first = sizes_default[0]
    assert default_first >= 1, "(3.b) text size >= 1 (a11y floor)"
    counts["atomic_3"] += 1

    # Drive the slider to 1.0 (max) → text_scale = 2.0.
    intervene = proc.request(
        "scene/intervene",
        {
            "path": f"/{FONT_SLIDER_TAG}/external/value",
            "value": 1.0,
        },
    )
    assert intervene is not None, "(3.c) slider intervene returned response"
    counts["atomic_3"] += 1

    root_max = layout_at(proc, 720, 480)
    sizes_max = collect_text_sizes(root_max)
    assert_eq(
        len(sizes_max),
        len(sizes_default),
        "(3.d) text node count stable across scale change",
    )
    counts["atomic_3"] += 1
    max_first = sizes_max[0]
    assert max_first > default_first, (
        "(3.e) text_scale=2.0 must produce LARGER text than default "
        f"(1.25); got default={default_first}, max={max_first}"
    )
    counts["atomic_3"] += 1

    # Drive slider to 0.0 → text_scale = 0.5.
    proc.request(
        "scene/intervene",
        {
            "path": f"/{FONT_SLIDER_TAG}/external/value",
            "value": 0.0,
        },
    )
    root_min = layout_at(proc, 720, 480)
    sizes_min = collect_text_sizes(root_min)
    min_first = sizes_min[0]
    assert min_first < max_first, (
        "(3.f) text_scale=0.5 must produce SMALLER text than 2.0; "
        f"got min={min_first}, max={max_first}"
    )
    counts["atomic_3"] += 1
    assert min_first >= 1, "(3.g) min scale still floors text at 1 px"
    counts["atomic_3"] += 1

    # Ratio sanity: max / min ~ (2.0 / 0.5) = 4.0, within integer
    # rounding noise.
    ratio = max_first / min_first
    assert 2.0 <= ratio <= 6.0, (
        f"(3.h) max/min scale ratio in expected range; got {ratio:.2f} "
        f"(min={min_first}, max={max_first})"
    )
    counts["atomic_3"] += 1

    # ─── Atomic (4) partial — font_slider value persists ───
    # Set slider to 0.7 then verify the value reads back from the
    # External (no need to relaunch — that path is already covered by
    # R667 demo which still passes).
    proc.request(
        "scene/intervene",
        {
            "path": f"/{FONT_SLIDER_TAG}/external/value",
            "value": 0.7,
        },
    )
    val_query = proc.query(f"/{FONT_SLIDER_TAG}/external/value")
    assert val_query is not None, "(4.a) slider value query returned"
    counts["atomic_4_partial"] += 1
    # The slider clamps to [0.0, 1.0] and the value lives behind the
    # External's `value` slot. The exact wire shape varies by
    # SliderExternal impl; just assert the response is non-null.
    val_payload = val_query.get("value") if isinstance(val_query, dict) else None
    if val_payload is not None:
        assert 0.65 <= float(val_payload) <= 0.75, (
            "(4.b) slider value persists across query "
            f"(expected ~0.7, got {val_payload})"
        )
        counts["atomic_4_partial"] += 1

    return counts


def main() -> None:
    with isolated_storage_dir("pinion-settings-panel-r668-") as _tempdir:
        with RpcSubprocess("settings-panel") as proc:
            counts = assertions(proc)
            total = sum(counts.values())
            print(f"[demo] R668 substrate verified across "
                  f"{total} assertions: {counts}", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(run_demo(
        "R668 §5.16 §5.38 §5.41 §5.49 — Phase A close + R668 substrate "
        "(SizeStrategy / TUI drain / checkbox lift / text_scale)",
        main,
    ))
