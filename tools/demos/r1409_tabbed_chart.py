#!/usr/bin/env python3
"""R1409 §5.16 §5.49 §5.51 §2 #2 #7 — a chart lives in a dock TAB well, over RPC.

R1396 (`hello-dock-chart`) proved a `LineChart::build_fill` re-scales inside a
dock **Leaf** pane. The `pinion-chart` debt note then flagged the case a real
dashboard needs — a chart as one **tab** of a `DockNode::Tabs` well — as
UNEXERCISED, with the caveat "the walker wraps a tab-well leaf identically, so it
is a demo gap, not a suspected defect." That caveat is a hypothesis: a `Tabs`
well is NOT byte-identical to a `Leaf` — it renders ONLY its active panel,
header-suppressed, below a fixed tab strip. `examples/hello-tabbed-chart` is the
forcing consumer, and this demo turns the hypothesis into an end-to-end proof
WITHOUT pixels:

  * Resize the window / drag the splitter while the chart tab is active: the well
    cell changes, the chart pane's measured rect (BELOW the strip) republishes,
    and the chart re-scales — the R1396 seam, now one wrapper (the tab well)
    deeper. A narrow well still collapses its legend to a `+N` marker (the R1396
    clamp) instead of bleeding over the readout.
  * Switch tabs (`scene/invoke` `send` — the real click wire): activating the
    notes tab REMOVES the chart tag from the scene (active-only render);
    re-activating the chart tab makes it reappear and re-measure. If the well was
    resized while the chart tab was hidden, the reappeared chart fits the NOW
    size, not the stale one — the reappear -> publish -> dirty -> re-pass chain a
    Leaf never exercises.

  (A) boot 820x500 — the well hosts the chart tab active; the chart is measured;
      the readout names the visible chart tab; `active` reads 0; both tab tags are
      painted; the legend is complete; every x-tick label is contained.
  (B) resize wide 1180x500 — the chart widens; legend complete; contained.
  (C) resize narrow 380x500 — the chart narrows; legend COLLAPSES to `+N`;
      containment STILL holds (the clamp, not an overrun).
  (D) settle 820x500, DRAG the splitter left — the well narrows independently of
      the (unchanged) window; the chart re-scales; the readout ratio changes.
  (E) switch to the NOTES tab — `active` reads 1; the chart tag LEAVES the scene
      (active-only render); the notes body enters it; the readout says hidden.
  (F) HEADLINE — while the chart tab is HIDDEN, resize the window narrow (nothing
      measures the absent chart, its size goes stale), then switch BACK to the
      chart tab: it reappears and re-measures to the NOW-narrow pane, not the
      stale wide one.

Run from the workspace root:
    cargo build -p hello-tabbed-chart --release
    python3 tools/demos/r1409_tabbed_chart.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    rect_of,
    run_demo,
    wait_until,
)

CHART_TAG = "chart"
CHART_SPLIT = "chart_split"
WELL = "left_well"
READOUT_BODY = "readout_body"
NOTES_BODY = "notes_body"


def snap_at(tf: RpcSubprocess, size: tuple[int, int]):
    return tf.snapshot(source="paint", viewport=size)


def chart_node(snap):
    return find_by_tag(snap, CHART_TAG)


def chart_rect(snap) -> dict:
    node = chart_node(snap)
    assert node is not None, "the chart root is present in the well"
    return rect_of(node)


def x_label_count(snap) -> int:
    k = 0
    while find_by_tag(snap, f"{CHART_TAG}.label.x.{k}") is not None:
        k += 1
    return k


def legend_label_count(snap) -> int:
    k = 0
    while find_by_tag(snap, f"{CHART_TAG}.legend.{k}.label") is not None:
        k += 1
    return k


def has_overflow(snap) -> bool:
    return find_by_tag(snap, f"{CHART_TAG}.legend.overflow") is not None


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
            f"edge {chart_right} — it would paint over the readout pane"
        )
    first = rect_of(find_by_tag(snap, f"{CHART_TAG}.label.x.0"))
    assert float(first["x"]) >= float(cr["x"]) - 0.5, (
        f"{label}: the first x-tick label starts left of the chart"
    )
    return n


def readout_text(snap) -> str:
    node = find_by_tag(snap, READOUT_BODY)
    assert node is not None, "the readout body is present"
    return node.get("content") or ""


def active_index(tf: RpcSubprocess) -> int:
    """The live active-tab index, read from the `TabWellExternal`'s §2 #7
    introspection — the topology-owned SSOT, not a derived paint read."""
    return int(tf.query(f"/{WELL}/external/active"))


def access_nodes(tf: RpcSubprocess) -> list[dict]:
    """The §2 #7 accessibility tree — what an AT / AI discovers about the surface."""
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    nodes = resp.result.get("nodes")
    assert isinstance(nodes, list), f"scene/access.nodes is a list; got {resp.result!r}"
    return nodes


def by_role(nodes: list[dict], role: str) -> list[dict]:
    return [n for n in nodes if n.get("role") == role]


def selected_tab_index(nodes: list[dict]) -> int:
    """The index of the aria-selected tab in the announced tablist (-1 if none)."""
    for i, t in enumerate(by_role(nodes, "tab")):
        if t.get("selected"):
            return i
    return -1


def switch_tab(tf: RpcSubprocess, i: int) -> None:
    """Click tab `i` over the R51.42 synthetic-event wire: a `PointerDown` records
    the pressed tab, the `PointerUp` release activates it (the click edge)."""
    tf.invoke(f"/{WELL}/external/send", f"{i}:PointerDown")
    tf.invoke(f"/{WELL}/external/send", f"{i}:PointerUp")


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
    """Drive a real `scene/resize`, then poll `from=paint` until the well's chart
    has re-scaled in the expected direction — zero-flake by outcome."""
    resp = tf.request("scene/resize", {"width": w, "height": h})
    assert resp is not None and resp.result is not None, "scene/resize accepted"

    def settled() -> bool:
        cur = chart_rect(snap_at(tf, (w, h)))["w"]
        return cur > prev_w if grow else cur < prev_w

    wait_until(settled, desc=f"the well's chart re-scales after resize to {w}x{h}")


def body() -> None:
    with RpcSubprocess("hello-tabbed-chart", boot_grace=2.0) as tf:
        checks = 0

        # ── (A) boot ────────────────────────────────────────────────────
        boot = (820, 500)
        snap = snap_at(tf, boot)
        assert active_index(tf) == 0, "(A) the chart tab is active at boot"
        checks += 1
        # Both tabs are painted in the strip (R51.42 `{well}#{i}` tab tags).
        assert find_by_tag(snap, f"{WELL}#0") is not None, "(A) tab 0 painted"
        checks += 1
        assert find_by_tag(snap, f"{WELL}#1") is not None, "(A) tab 1 painted"
        checks += 1
        assert find_by_tag(snap, f"{CHART_TAG}.series.0") is not None, (
            "(A) the chart tab painted a series polyline"
        )
        checks += 1
        assert find_by_tag(snap, NOTES_BODY) is None, (
            "(A) the notes tab is inactive, so its body is absent"
        )
        checks += 1
        boot_rect = chart_rect(snap)
        assert boot_rect["w"] > 0 and boot_rect["h"] > 0, (
            f"(A) the chart measured a real rect below the strip: {boot_rect}"
        )
        checks += 1
        rtext = readout_text(snap)
        assert "chart tab visible" in rtext and "index 0" in rtext, (
            f"(A) the readout names the visible chart tab: {rtext!r}"
        )
        checks += 1
        n_labels = assert_labels_contained(snap, "(A)")
        checks += n_labels
        assert not has_overflow(snap), "(A) a comfortable well shows the full legend"
        checks += 1
        assert legend_label_count(snap) == 4, "(A) all four series in the legend"
        checks += 1
        # §2 #7 — the tab well announces itself, so an AI can DISCOVER the well +
        # its tabs (not just see the chart pixels). A chart in a tab an AI cannot
        # enumerate would violate the "AI-introspection 1st-class" invariant.
        nodes = access_nodes(tf)
        tablists = by_role(nodes, "tablist")
        assert len(tablists) == 1, "(A) exactly one tablist is announced"
        checks += 1
        assert tablists[0].get("tag") == WELL, "(A) the tablist is tagged with the well id"
        checks += 1
        assert len(by_role(nodes, "tab")) == 2, "(A) one announced tab per panel"
        checks += 1
        assert selected_tab_index(nodes) == 0, "(A) the chart tab is aria-selected at boot"
        checks += 1
        boot_w = float(boot_rect["w"])

        # ── (B) resize wide ─────────────────────────────────────────────
        resize_and_settle(tf, 1180, 500, boot_w, grow=True)
        snap = snap_at(tf, (1180, 500))
        wide_w = float(chart_rect(snap)["w"])
        assert wide_w > boot_w, f"(B) a wider window widens the chart: {boot_w} -> {wide_w}"
        checks += 1
        assert not has_overflow(snap), "(B) the wide legend stays complete"
        checks += 1
        assert legend_label_count(snap) == 4, "(B) all four legend entries at wide"
        checks += 1
        checks += assert_labels_contained(snap, "(B)")

        # ── (C) resize narrow ───────────────────────────────────────────
        resize_and_settle(tf, 380, 500, wide_w, grow=False)
        snap = snap_at(tf, (380, 500))
        narrow_w = float(chart_rect(snap)["w"])
        assert narrow_w < boot_w, f"(C) a narrow window narrows the chart: {boot_w} -> {narrow_w}"
        checks += 1
        assert has_overflow(snap), (
            "(C) the narrow well COLLAPSES its legend to a `+N` marker"
        )
        checks += 1
        assert legend_label_count(snap) < 4, "(C) fewer legend entries when narrow"
        checks += 1
        checks += assert_labels_contained(snap, "(C)")

        # ── (D) splitter drag (dock-native resize) ──────────────────────
        resize_and_settle(tf, 820, 500, narrow_w, grow=True)
        settled = (820, 500)
        pre = snap_at(tf, settled)
        pre_w = float(chart_rect(pre)["w"])
        pre_text = readout_text(pre)

        hx, hy = split_handle_center(tf, settled)
        tf.drag(from_at=(hx, hy), to_at=(hx - 200.0, hy))

        def pane_shrank() -> bool:
            return float(chart_rect(snap_at(tf, settled))["w"]) < pre_w - 10.0

        wait_until(pane_shrank, desc="dragging the splitter left narrows the well")
        post = snap_at(tf, settled)
        post_w = float(chart_rect(post)["w"])
        assert post_w < pre_w, (
            f"(D) the splitter drag narrowed the well at a FIXED window: {pre_w} -> {post_w}"
        )
        checks += 1
        assert find_by_tag(post, f"{CHART_TAG}.series.0") is not None, (
            "(D) the chart still paints its series after the splitter drag"
        )
        checks += 1
        assert readout_text(post) != pre_text, (
            f"(D) the readout reflects the new split ratio: {pre_text!r} -> {readout_text(post)!r}"
        )
        checks += 1
        checks += assert_labels_contained(post, "(D)")

        # ── (E) switch to the NOTES tab (active-only render) ─────────────
        switch_tab(tf, 1)
        wait_until(
            lambda: chart_node(snap_at(tf, settled)) is None,
            desc="activating the notes tab removes the chart tag from the scene",
        )
        snap = snap_at(tf, settled)
        assert active_index(tf) == 1, "(E) the notes tab is now active"
        checks += 1
        assert chart_node(snap) is None, (
            "(E) a Tabs well renders only the active panel — the chart tag is absent"
        )
        checks += 1
        assert find_by_tag(snap, NOTES_BODY) is not None, "(E) the notes body is in the scene"
        checks += 1
        assert "chart tab hidden" in readout_text(snap), (
            f"(E) the readout names the hidden chart tab: {readout_text(snap)!r}"
        )
        checks += 1
        # §2 #7 — the tab switch moved aria-selected too, so an AT tracks the active
        # tab exactly as the painted strip does.
        assert selected_tab_index(access_nodes(tf)) == 1, (
            "(E) aria-selected moved to the notes tab"
        )
        checks += 1

        # ── (F) HEADLINE — resize while hidden, catch up on reactivation ─
        # Establish a WIDE reference (chart active, measured wide, full legend), then
        # HIDE the chart, resize the window NARROW while it is absent (nothing
        # measures it — its size signal goes stale wide), then switch BACK. The
        # reappeared chart must REBUILD from the now-narrow measured pane, not the
        # stale wide size.
        switch_tab(tf, 0)
        resize_and_settle(tf, 1180, 500, post_w, grow=True)
        wide = snap_at(tf, (1180, 500))
        ref_wide_w = float(chart_rect(wide)["w"])
        assert not has_overflow(wide), "(F) the wide reference chart shows the full legend"
        checks += 1

        switch_tab(tf, 1)  # hide the chart
        wait_until(
            lambda: chart_node(snap_at(tf, (1180, 500))) is None,
            desc="the chart tag leaves the scene again",
        )
        # Resize the WINDOW narrow while the chart is hidden — its size goes stale.
        resp = tf.request("scene/resize", {"width": 360, "height": 500})
        assert resp is not None and resp.result is not None, "(F) scene/resize accepted"

        switch_tab(tf, 0)  # re-activate the chart into the now-narrow pane

        # DISCRIMINATING: the chart-ROOT width is trivially narrow (fill-parent =
        # pane width) whether or not the internal geometry re-measured, so a
        # root-width check proves nothing. Wait on — and assert — the (cw,ch)-DRIVEN
        # internal state instead: the reappeared chart's legend COLLAPSES to a `+N`
        # marker and its x-tick labels stay CONTAINED, both of which happen only if
        # `build_body` used the NARROW measured size. A chart rebuilt from the stale
        # wide size would keep the full legend and overflow the narrow pane.
        def reappeared_and_rebuilt_narrow() -> bool:
            s = snap_at(tf, (360, 500))
            return chart_node(s) is not None and has_overflow(s)

        wait_until(
            reappeared_and_rebuilt_narrow,
            desc="the reappeared chart REBUILDS at the now-narrow pane (legend collapses), not the stale wide one",
        )
        snap = snap_at(tf, (360, 500))
        assert active_index(tf) == 0, "(F) the chart tab is active again"
        checks += 1
        assert chart_node(snap) is not None, (
            "(F) the chart reappeared when its tab was re-activated"
        )
        checks += 1
        assert has_overflow(snap), (
            "(F) the reappeared narrow chart COLLAPSED its legend — its internal "
            "geometry rebuilt from the NARROW pane, not the stale wide size"
        )
        checks += 1
        assert legend_label_count(snap) < 4, (
            "(F) fewer legend entries when narrow (not the stale wide full legend)"
        )
        checks += 1
        # Every x-tick label stays inside the narrow chart (the R1396 clamp) — this
        # FAILS if the re-pass authored the axis at the stale wide extent.
        checks += assert_labels_contained(snap, "(F)")
        # Sanity: the chart-root pane is narrower than the wide reference.
        back_w = float(chart_rect(snap)["w"])
        assert back_w < ref_wide_w, (
            f"(F) the reappeared chart pane is narrower than the wide reference: "
            f"{ref_wide_w} -> {back_w}"
        )
        checks += 1
        assert find_by_tag(snap, f"{CHART_TAG}.series.0") is not None, (
            "(F) the reappeared chart is a real, non-empty chart"
        )
        checks += 1

        print(
            f"[demo] ok — a chart in a dock TAB well: measured, resized, tab-switched, "
            f"and re-measured on reactivation, {checks} assertions"
        )


if __name__ == "__main__":
    raise SystemExit(run_demo("r1409_tabbed_chart", body))
