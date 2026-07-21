#!/usr/bin/env python3
"""R1396 §5.16 §5.49 §2 #2 #7 — a chart lives in a resizable DOCK PANE, over RPC.

The `pinion-chart` crate's own debt note flagged the docked-chart case — a
`build_fill` chart inside a `view_dock_surface` pane, keyed on
`use_pane_viewport_size` of a tag nested two containers deep in the dock — as
UNPROVEN ("the dock<->pane-registry<->chart-tag interaction is untested. Do not
assert it."). `examples/hello-dock-chart` is the missing consumer, and this demo
proves the seam end to end WITHOUT pixels:

  * Resize the window (`scene/resize`): the chart pane re-lays-out, its measured
    rect republishes under the chart tag, and the chart re-scales — the same
    live-paint publish `hello-chart-fill` uses, now for a dock-nested tag.
  * Drag the splitter (`scene/drag` on the handle): the chart pane resizes
    INDEPENDENTLY of the window and the chart re-scales to it — the dock-native
    resize the window path cannot exercise.

It is also the forcing consumer for R1396's narrow-pane clamp: a window can
absorb a chart's legend + last-tick overhang in its own padding, a dock pane
cannot. So this demo drags the chart pane narrow and asserts the legend collapses
to a tagged `+N` marker (`chart.legend.overflow`) and every x-tick label stays
inside the chart's own rect — instead of bleeding over the readout pane.

  (A) boot 760x480 — the dock hosts a tagged chart in its left pane; the readout
      names the measured pane; every x-tick label is contained; the legend is
      complete (all four series, no overflow marker).
  (B) resize wide 1120x480 — the chart widens; the legend stays complete;
      containment holds.
  (C) resize narrow 360x480 — the chart narrows; the legend COLLAPSES to a `+N`
      marker; containment STILL holds (the clamp, not an overrun).
  (D) settle 760x480, then DRAG the splitter left — the chart pane narrows
      independently of the (unchanged) window and the chart re-scales; the split
      ratio in the readout drops. The dock-native resize path.

Run from the workspace root:
    cargo build -p hello-dock-chart --release
    python3 tools/demos/r1396_dock_chart.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    count_indexed_tags,
    find_by_tag,
    rect_of,
    run_demo,
    wait_until,
)

CHART_TAG = "chart"
CHART_SPLIT = "chart_split"
CHART_PANEL = "chart-pane"
READOUT_PANEL = "readout-pane"
READOUT_BODY = "readout_body"


def snap_at(tf: RpcSubprocess, size: tuple[int, int]):
    return tf.snapshot(source="paint", viewport=size)


def chart_rect(snap) -> dict:
    node = find_by_tag(snap, CHART_TAG)
    assert node is not None, "the chart root is present in the dock pane"
    return rect_of(node)


def x_label_count(snap) -> int:
    """How many `chart.label.x.{k}` tick labels the chart painted."""
    return count_indexed_tags(snap, f"{CHART_TAG}.label.x.")


def legend_label_count(snap) -> int:
    return count_indexed_tags(snap, f"{CHART_TAG}.legend.", ".label")


def has_overflow(snap) -> bool:
    return find_by_tag(snap, f"{CHART_TAG}.legend.overflow") is not None


def assert_labels_contained(snap, label: str) -> int:
    """Every x-tick label box must end at or before the chart's right edge — the
    R1396 clamp. Returns the number of labels checked (>0, so the assertion is
    not vacuous)."""
    cr = chart_rect(snap)
    chart_right = float(cr["x"]) + float(cr["w"])
    n = x_label_count(snap)
    assert n > 0, f"{label}: the chart paints x-tick labels"
    for k in range(n):
        node = find_by_tag(snap, f"{CHART_TAG}.label.x.{k}")
        r = rect_of(node)
        right = float(r["x"]) + float(r["w"])
        assert right <= chart_right + 0.5, (
            f"{label}: x-tick label {k} ends at {right} past the chart's right "
            f"edge {chart_right} — it would paint over the neighbouring pane"
        )
    # The left edge of the first label is also inside the chart.
    first = rect_of(find_by_tag(snap, f"{CHART_TAG}.label.x.0"))
    assert float(first["x"]) >= float(cr["x"]) - 0.5, (
        f"{label}: the first x-tick label starts left of the chart"
    )
    return n


def readout_text(snap) -> str:
    node = find_by_tag(snap, READOUT_BODY)
    assert node is not None, "the readout body is present"
    return node.get("content") or ""


def split_handle_center(tf: RpcSubprocess, size: tuple[int, int]) -> tuple[float, float]:
    """Centre of the splitter's drag handle — the untagged middle child of the
    `chart_split` container (the R685 / R1346 handle idiom)."""
    snap = snap_at(tf, size)
    node = find_by_tag(snap, CHART_SPLIT)
    assert node is not None, "the split container carries its id tag"
    children = node.get("children") or []
    assert len(children) == 3, (
        f"the splitter paints [first, handle, second]; got {len(children)}"
    )
    handle = children[1]
    assert handle.get("tag") is None, "the handle stays untagged (R685)"
    r = rect_of(handle)
    return float(r["x"]) + float(r["w"]) / 2.0, float(r["y"]) + float(r["h"]) / 2.0


def resize_and_settle(tf: RpcSubprocess, w: int, h: int, prev_w: float, grow: bool):
    """Drive a real `scene/resize`, then poll `from=paint` until the docked chart
    has re-scaled in the expected direction — zero-flake by outcome."""
    resp = tf.request("scene/resize", {"width": w, "height": h})
    assert resp is not None and resp.result is not None, "scene/resize accepted"

    def settled() -> bool:
        cur = chart_rect(snap_at(tf, (w, h)))["w"]
        return cur > prev_w if grow else cur < prev_w

    wait_until(settled, desc=f"docked chart re-scales after resize to {w}x{h}")


def body() -> None:
    with RpcSubprocess("hello-dock-chart", boot_grace=2.0) as tf:
        checks = 0

        # ── (A) boot ────────────────────────────────────────────────────
        boot = (760, 480)
        snap = snap_at(tf, boot)
        assert find_by_tag(snap, CHART_PANEL) is not None, "(A) chart pane present"
        checks += 1
        assert find_by_tag(snap, READOUT_PANEL) is not None, "(A) readout pane present"
        checks += 1
        assert find_by_tag(snap, f"{CHART_TAG}.series.0") is not None, (
            "(A) the docked chart painted a series polyline"
        )
        checks += 1
        boot_rect = chart_rect(snap)
        assert boot_rect["w"] > 0 and boot_rect["h"] > 0, (
            f"(A) the chart measured a real rect in its pane: {boot_rect}"
        )
        checks += 1
        assert "measured" in readout_text(snap), (
            f"(A) the readout names the measured pane: {readout_text(snap)!r}"
        )
        checks += 1
        n_labels = assert_labels_contained(snap, "(A)")
        checks += n_labels  # one containment check per label
        assert not has_overflow(snap), "(A) a comfortable pane shows the full legend"
        checks += 1
        boot_legend = legend_label_count(snap)
        assert boot_legend == 4, f"(A) all four series in the legend, got {boot_legend}"
        checks += 1
        boot_w = float(boot_rect["w"])

        # ── (B) resize wide ─────────────────────────────────────────────
        resize_and_settle(tf, 1120, 480, boot_w, grow=True)
        snap = snap_at(tf, (1120, 480))
        wide_w = float(chart_rect(snap)["w"])
        assert wide_w > boot_w, f"(B) a wider window widens the chart: {boot_w} -> {wide_w}"
        checks += 1
        assert not has_overflow(snap), "(B) the wide legend stays complete"
        checks += 1
        assert legend_label_count(snap) == 4, "(B) all four legend entries at wide"
        checks += 1
        checks += assert_labels_contained(snap, "(B)")

        # ── (C) resize narrow ───────────────────────────────────────────
        resize_and_settle(tf, 360, 480, wide_w, grow=False)
        snap = snap_at(tf, (360, 480))
        narrow_w = float(chart_rect(snap)["w"])
        assert narrow_w < boot_w, f"(C) a narrow window narrows the chart: {boot_w} -> {narrow_w}"
        checks += 1
        assert has_overflow(snap), (
            "(C) the narrow pane COLLAPSES its legend to a `+N` marker "
            "(instead of overrunning the readout pane)"
        )
        checks += 1
        assert legend_label_count(snap) < 4, (
            f"(C) fewer legend entries are drawn when narrow, got {legend_label_count(snap)}"
        )
        checks += 1
        # The clamp, not an overrun: even at 360px every x-tick label is inside
        # the chart's own rect.
        checks += assert_labels_contained(snap, "(C)")

        # ── (D) splitter drag (dock-native resize) ──────────────────────
        # Settle back to a fixed size, then drag the divider — the WINDOW does
        # not change, only the pane split, so this exercises the dock path the
        # window resize cannot.
        resize_and_settle(tf, 760, 480, narrow_w, grow=True)
        settled = (760, 480)
        pre = snap_at(tf, settled)
        pre_w = float(chart_rect(pre)["w"])
        pre_text = readout_text(pre)

        hx, hy = split_handle_center(tf, settled)
        tf.drag(from_at=(hx, hy), to_at=(hx - 200.0, hy))

        def pane_shrank() -> bool:
            return float(chart_rect(snap_at(tf, settled))["w"]) < pre_w - 10.0

        wait_until(pane_shrank, desc="dragging the splitter left narrows the chart pane")
        post = snap_at(tf, settled)
        post_w = float(chart_rect(post)["w"])
        assert post_w < pre_w, (
            f"(D) the splitter drag narrowed the chart pane at a FIXED window: "
            f"{pre_w} -> {post_w}"
        )
        checks += 1
        # The chart is still a real, non-empty chart after the pane resize.
        assert find_by_tag(post, f"{CHART_TAG}.series.0") is not None, (
            "(D) the chart still paints its series after the splitter drag"
        )
        checks += 1
        # The readout's ratio text changed (the split moved).
        assert readout_text(post) != pre_text, (
            f"(D) the readout reflects the new split ratio: {pre_text!r} -> {readout_text(post)!r}"
        )
        checks += 1
        # Containment holds at the dragged-narrow width too.
        checks += assert_labels_contained(post, "(D)")

        print(f"[demo] ok — docked chart seam + narrow-pane clamp proven, {checks} assertions")


if __name__ == "__main__":
    raise SystemExit(run_demo("r1396_dock_chart", body))
