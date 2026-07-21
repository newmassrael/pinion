#!/usr/bin/env python3
"""R1410 §5.16 §5.51 §5.49 §5.23 §2 #2 #7 — a chart re-measures in a torn-off
(floating) OS window, over RPC.

R1396 (`hello-dock-chart`) proved a `LineChart::build_fill` re-scales inside a dock
**pane of one window**; R1409 (`hello-tabbed-chart`) proved the same inside a
`Tabs` **well**. The `pinion-chart` debt note flagged the LAST placement a real
dashboard needs — a chart torn off into its OWN floating window — as UNEXERCISED.
Separately `pane_viewport_seam.rs` (R1021) proved a *plain* pane reflows to its
floating window's size, but NOTHING crossed the two: a chart's `build_fill`
measured inside a floating window. `examples/hello-floating-chart` is that missing
consumer, and this demo turns it into an end-to-end proof WITHOUT pixels:

  * The chart panel tears off via the AI `invoke("tear_off")` toggle: the shell's
    R683 reconcile Effect spawns a `torn-chartpane` window (async), and its own
    per-window `publish_pane_viewports` measures `CHART_TAG` in THAT window, so the
    chart reflows to the floating window's size — SHORTER than the docked pane
    (FLOAT_H 360 < WIN_H 480). The main dock drops the chart to a placeholder, so
    the chart tag is drawn in exactly one window per frame (the R1021.1 precondition).
  * Docking back (`invoke("tear_off")` again) drops the window and re-installs the
    chart in the main dock, re-measured to the docked pane.

  (A) boot 760x480 — the main window hosts the DOCKED chart; it is measured; the
      readout says DOCKED; every x-tick label is contained; the legend is present;
      `scene/windows` lists ONLY main.
  (B) tear off — `invoke("/chartpane/external/tear_off")`; the `torn-chartpane`
      window becomes addressable; `scene/windows` gains it; the main scene LOSES
      the chart tag (a placeholder takes its slot); the readout says FLOATING.
  (C) floating measure — snapshot `{window: torn-chartpane}`: the chart is present;
      its x-tick labels fit inside the floating chart's SHORTER bottom edge — the
      DISCRIMINATING witness that the internal geometry was authored from the
      floating measurement (a stale taller-docked read would push the labels below
      the shorter chart's bottom; a (0,0) read would paint none). The root-rect
      magnitudes (shorter + wider than docked) are reported too, but they are
      fill-parent and would hold even without the publish, so they are informative,
      not the proof. (A spawned window ignores a `scene/snapshot` viewport override
      and there is no window-scoped `scene/resize` RPC, so the DEMO fixes the
      floating window at one size; the multi-size re-measure — 360 vs 560 — is
      pinned by the crate's own `ShellCore::compute_paint_scene_for_window` tests.)
  (D) dock back — `invoke("tear_off")` again; the floating window becomes
      un-addressable; `scene/windows` loses it; the main scene REGAINS the chart,
      re-measured to the docked pane; the readout says DOCKED.
  (E) determinism — back-to-back reads agree; a second float/dock cycle is idempotent.

Run from the workspace root:
    cargo build -p hello-floating-chart --release
    python3 tools/demos/r1410_floating_chart.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    count_indexed_tags,
    find_by_tag,
    rect_of,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-floating-chart"
CHART_TAG = "chart"
READOUT_BODY = "readout_body"
CHART_PANEL = "chartpane"

MAIN = "main"
FLOAT_WIN = "torn-chartpane"

MAIN_W, MAIN_H = 760, 480
FLOAT_W, FLOAT_H = 560, 360


def snap_main(tf: RpcSubprocess, size=(MAIN_W, MAIN_H)):
    return tf.snapshot(source="paint", viewport=size, window=MAIN)


def snap_float(tf: RpcSubprocess, size=(FLOAT_W, FLOAT_H)):
    """Window-scoped snapshot of the floating chart window. Raises `RpcError`
    while the window does not exist (docked / not yet reconciled)."""
    return tf.snapshot(source="paint", viewport=size, window=FLOAT_WIN)


def chart_rect(snap) -> dict:
    node = find_by_tag(snap, CHART_TAG)
    assert node is not None, "the chart root is present"
    return rect_of(node)


def x_label_count(snap) -> int:
    return count_indexed_tags(snap, f"{CHART_TAG}.label.x.")


def legend_label_count(snap) -> int:
    return count_indexed_tags(snap, f"{CHART_TAG}.legend.", ".label")


def assert_labels_contained(snap, label: str) -> int:
    """Every x-tick label box ends at or before the chart's right edge (the R1396
    clamp). Returns the number of labels checked (>0, so it is not vacuous)."""
    cr = chart_rect(snap)
    chart_right = float(cr["x"]) + float(cr["w"])
    n = x_label_count(snap)
    assert n > 0, f"{label}: the chart paints x-tick labels"
    for k in range(n):
        r = rect_of(find_by_tag(snap, f"{CHART_TAG}.label.x.{k}"))
        right = float(r["x"]) + float(r["w"])
        assert right <= chart_right + 0.5, (
            f"{label}: x-tick label {k} ends at {right} past the chart's right "
            f"edge {chart_right} — the internal geometry was not authored from "
            f"this window's measurement"
        )
    return n


def float_geometry_settled(snap) -> bool:
    """Is the floating chart's `(cw, ch)`-driven INTERNAL geometry self-consistent
    with its own frame — every x-tick label inside BOTH the chart's right and bottom
    edges, and at least one label present? This is the SETTLE + DISCRIMINATING
    condition. The torn-off window spawns and re-measures ASYNCHRONOUSLY (its first
    addressable paint can still carry a stale-size internal build before the
    same-frame re-pass lands), so the demo waits for this to hold. A working
    per-window publish reaches it; a BROKEN one never does — a `(0,0)` read paints no
    labels, and a stale TALLER-docked read pushes the labels below the shorter
    floating chart's bottom (the paint adapter does not clip, R1356). So the wait
    itself is the proof: it TIMES OUT (a demo failure) iff the floating window's own
    publish did not re-measure the chart."""
    cr = chart_rect(snap)
    right = float(cr["x"]) + float(cr["w"])
    bottom = float(cr["y"]) + float(cr["h"])
    n = x_label_count(snap)
    if n == 0:
        return False
    for k in range(n):
        r = rect_of(find_by_tag(snap, f"{CHART_TAG}.label.x.{k}"))
        if float(r["x"]) + float(r["w"]) > right + 0.5:
            return False
        if float(r["y"]) + float(r["h"]) > bottom + 0.5:
            return False
    return True


def wait_float_settled(tf: RpcSubprocess):
    """Poll the floating window until its chart's internal geometry has settled inside
    its frame (see [`float_geometry_settled`]); return that settled snapshot. Times
    out (a hard failure) if the floating window's per-window publish never
    re-measures the chart — the demo's discriminating proof, by outcome."""
    def poll():
        try:
            snap = snap_float(tf)
        except RpcError:
            return None
        return snap if float_geometry_settled(snap) else None

    return wait_until(poll, desc="the floating chart re-measures + settles inside its frame")


def readout_text(snap) -> str:
    node = find_by_tag(snap, READOUT_BODY)
    assert node is not None, "the readout body is present"
    return node.get("content") or ""


def window_ids(tf: RpcSubprocess) -> set:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    return {w.get("id") for w in (resp.result.get("windows") or [])}


def tear_off(tf: RpcSubprocess) -> None:
    """Toggle the chart's float state through the AI `invoke("tear_off")` channel
    (the DockPanelExternal's cursor-less toggle — the same intent a header-escape
    drag emits)."""
    tf.invoke(f"/{CHART_PANEL}/external/tear_off", None)


def wait_float_addressable(tf: RpcSubprocess):
    """Gate on the torn-off window becoming RPC-addressable — the reconcile Effect
    spawns it AFTER the invoke returns (R883 zero-flake)."""
    def poll():
        try:
            return snap_float(tf)
        except RpcError:
            return None

    return wait_until(poll, desc=f"floating window {FLOAT_WIN} addressable")


def wait_float_gone(tf: RpcSubprocess) -> None:
    """Gate on the torn-off window DISAPPEARING after a dock-back."""
    def gone() -> bool:
        try:
            snap_float(tf)
            return False
        except RpcError:
            return True

    wait_until(gone, desc=f"floating window {FLOAT_WIN} dropped")


def wait_main_has_chart(tf: RpcSubprocess, present: bool) -> None:
    """Poll the main window until the chart tag is present / absent (the placeholder
    swap converges after the reconcile fan-out)."""
    def matches() -> bool:
        return (find_by_tag(snap_main(tf), CHART_TAG) is not None) == present

    wait_until(matches, desc=f"main chart tag present={present}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=2.0) as tf:
        checks = 0

        # ── (A) Boot: the chart is DOCKED in the main window ──────────────
        snap = snap_main(tf)
        assert find_by_tag(snap, CHART_TAG) is not None, "A: chart docked in main"
        checks += 1
        dcr = chart_rect(snap)
        assert dcr["w"] > 0 and dcr["h"] > 0, f"A: docked chart measured {dcr}"
        checks += 1
        docked_w, docked_h = float(dcr["w"]), float(dcr["h"])
        txt = readout_text(snap)
        assert "DOCKED" in txt, f"A: readout says docked, got {txt!r}"
        checks += 1
        assert "measured" in txt, f"A: readout names the measured size, got {txt!r}"
        checks += 1
        n = assert_labels_contained(snap, "A")
        assert n > 0, "A: docked chart paints contained x-tick labels"
        checks += 1
        assert legend_label_count(snap) > 0, "A: docked chart paints a legend"
        checks += 1
        assert window_ids(tf) == {MAIN}, "A: only the main window exists at boot"
        checks += 1
        # The floating window is not addressable while the chart is docked.
        raised = False
        try:
            snap_float(tf)
        except RpcError:
            raised = True
        assert raised, "A: the torn window is not addressable while docked"
        checks += 1

        # ── (B) Tear off: a real floating window appears ──────────────────
        tear_off(tf)
        wait_float_addressable(tf)
        checks += 1
        assert FLOAT_WIN in window_ids(tf), "B: scene/windows gains the torn window"
        checks += 1
        wait_main_has_chart(tf, present=False)
        main_after = snap_main(tf)
        assert find_by_tag(main_after, CHART_TAG) is None, (
            "B: the chart tag LEFT the main scene (a placeholder takes its slot)"
        )
        checks += 1
        rtxt = readout_text(main_after)
        assert "FLOATING" in rtxt, "B: readout says floating"
        checks += 1
        assert FLOAT_WIN in rtxt, f"B: the readout names the torn window, got {rtxt!r}"
        checks += 1

        # ── (C) The floating chart re-measures to ITS window ──────────────
        # DISCRIMINATING (by outcome): wait for the floating chart's internal geometry
        # to settle inside its own frame. The torn-off window re-measures asynchronously,
        # and a BROKEN per-window publish never settles (a (0,0) read paints no labels;
        # a stale taller-docked read overflows the shorter chart's bottom) — so this
        # wait TIMES OUT iff the floating window did not re-measure the chart.
        fsnap = wait_float_settled(tf)
        checks += 1
        fcr = chart_rect(fsnap)
        assert fcr["w"] > 0 and fcr["h"] > 0, f"C: floating chart measured {fcr}"
        checks += 1
        float_w, float_h = float(fcr["w"]), float(fcr["h"])
        # Root-rect magnitudes (informative, NOT the discriminator — the chart root is
        # fill-parent, so these hold regardless of the publish; the settle wait above is
        # the real proof, and the deterministic per-size sweep is in the unit tests).
        assert float_h < docked_h and float_w > docked_w, (
            f"C: the floating chart root is shorter + wider than docked "
            f"(float {float_w}x{float_h} vs docked {docked_w}x{docked_h})"
        )
        checks += 1
        assert assert_labels_contained(fsnap, "C") > 0, (
            "C: the settled floating chart's x-tick labels fit its width (R1396 clamp)"
        )
        checks += 1
        assert legend_label_count(fsnap) > 0, "C: the floating chart paints a legend"
        checks += 1
        # The docked chart tag is still absent from main while floating (R1021.1:
        # the tag lives in exactly one window per frame).
        assert find_by_tag(snap_main(tf), CHART_TAG) is None, (
            "C: the chart tag stays in the floating window only, not main"
        )
        checks += 1

        # ── (D) Dock back: the window drops, the chart re-installs ────────
        tear_off(tf)
        wait_float_gone(tf)
        checks += 1
        assert FLOAT_WIN not in window_ids(tf), "D: scene/windows loses the torn window"
        checks += 1
        wait_main_has_chart(tf, present=True)
        back = snap_main(tf)
        rcr = chart_rect(back)
        assert rcr["w"] > 0 and rcr["h"] > 0, f"D: re-docked chart measured {rcr}"
        checks += 1
        assert "DOCKED" in readout_text(back), "D: readout says docked again"
        checks += 1
        assert abs(float(rcr["h"]) - docked_h) <= 1.0, (
            f"D: the re-docked chart re-measured to the docked pane "
            f"(h {rcr['h']} ~= {docked_h})"
        )
        checks += 1
        assert abs(float(rcr["w"]) - docked_w) <= 1.0, (
            f"D: the re-docked chart width returned to the docked pane "
            f"(w {rcr['w']} ~= {docked_w})"
        )
        checks += 1

        # ── (E) Determinism + idempotence ─────────────────────────────────
        again = chart_rect(snap_main(tf))
        assert again == rcr, "E: back-to-back main reads agree"
        checks += 1
        tear_off(tf)  # float again
        f2 = wait_float_settled(tf)
        assert float(chart_rect(f2)["h"]) < docked_h, "E: re-float re-measures short"
        checks += 1
        assert FLOAT_WIN in window_ids(tf), "E: the torn window is back"
        checks += 1
        tear_off(tf)  # dock back again
        wait_float_gone(tf)
        assert FLOAT_WIN not in window_ids(tf), "E: idempotent dock-back drops it"
        checks += 1
        assert window_ids(tf) == {MAIN}, "E: the final topology is the single main window"
        checks += 1

        print(f"r1410 OK ({checks} assertions)")
        assert checks >= 30, f"expected >= 30 assertions, ran {checks}"


if __name__ == "__main__":
    sys.exit(run_demo("r1410_floating_chart", body))
