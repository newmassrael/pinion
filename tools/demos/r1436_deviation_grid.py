#!/usr/bin/env python3
"""R1436 §5.35 — a DIVERGING colour scale over a deviation-from-baseline grid.

`pinion_chart::ColorScale` is the continuous peer of the categorical palette:
a sequential ramp answers "how big", a diverging ramp answers "how far, and
which way, from a meaningful zero". Its defining property is that the baseline
lands on the NEUTRAL colour exactly — and the trap is that real deviation data
is rarely symmetric. This grid runs -14..+32, so a linear map would paint the
neutral a third of the way up the ramp and a genuinely positive deviation would
read as "on target". `map_diverging` normalises each wing on its own width.

The demo proves that over the wire rather than in prose: the oracle publishes
`color_at` (the painted, diverging colour) AND `linear_color_at` (the same value
through the linear map), so this script reads the zero cell both ways and shows
only the diverging one is neutral.

It also asserts the ACCESSIBILITY floor as data: every cell's label is inked
with `readable_ink` (the higher-WCAG-contrast of two pinned inks, computed per
cell), and `min_contrast` publishes the worst ratio over the whole grid, so a
client verifies legibility with no pixel and no eyeballing.

Run from the workspace root:
    cargo build -p hello-deviation-grid --release
    python3 tools/demos/r1436_deviation_grid.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    RpcError,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

ROWS, COLS = 6, 10
CELL_W, CELL_H, CELL_GAP = 62, 40, 3
GRID_X, GRID_Y = 24, 64
WIN = (GRID_X * 2 + COLS * (CELL_W + CELL_GAP), GRID_Y + ROWS * (CELL_H + CELL_GAP) + 92)

GRID = "deviation"
EXT = "/external"
# Row 2 is pinned to exact zeros — the on-target row.
ZERO_ROW = 2


def cell_tag(r: int, c: int) -> str:
    return f"dev.cell.{r}.{c}"


def at(tf, path: str, cell: tuple[int, int]):
    return tf.invoke(f"{EXT}/{path}", f"{cell[0]},{cell[1]}")


def body() -> None:
    with RpcSubprocess("hello-deviation-grid") as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, cell_tag(ROWS - 1, COLS - 1)) is not None,
            source="paint",
            viewport=WIN,
            desc="every grid cell painted",
        )

        # --- the model is on the wire (no pixel needed). ---
        assert_eq(tf.query(f"{EXT}/rows"), ROWS, "rows published")
        assert_eq(tf.query(f"{EXT}/cols"), COLS, "cols published")
        lo = float(tf.query(f"{EXT}/min"))
        mid = float(tf.query(f"{EXT}/mid"))
        hi = float(tf.query(f"{EXT}/max"))
        assert lo < mid < hi, f"the domain brackets the baseline: {lo} < {mid} < {hi}"
        assert abs(mid) < 1e-9, f"the baseline is zero, got {mid}"
        # The whole point: the two wings are NOT the same width.
        assert abs((mid - lo) - (hi - mid)) > 1.0, (
            f"the domain must be asymmetric for this test to mean anything: {lo}..{hi}"
        )
        print(f"  ok: asymmetric domain {lo}..{hi} around {mid}")

        neutral = str(tf.query(f"{EXT}/neutral_hex"))
        assert neutral.startswith("#") and len(neutral) == 7, f"neutral hex: {neutral}"
        print(f"  ok: ramp neutral is {neutral}")

        # --- THE property: a zero cell is EXACTLY the neutral colour. ---
        for c in (0, COLS // 2, COLS - 1):
            value = float(at(tf, "value_at", (ZERO_ROW, c)))
            assert abs(value) < 1e-9, f"row {ZERO_ROW} col {c} is on target, got {value}"
            assert_eq(
                at(tf, "color_at", (ZERO_ROW, c)),
                neutral,
                f"the on-target cell ({ZERO_ROW},{c}) paints the neutral",
            )
        print(f"  ok: every on-target cell in row {ZERO_ROW} is the neutral colour")

        # --- the counter-proof: the LINEAR map is not neutral at zero. ---
        linear_zero = at(tf, "linear_color_at", (ZERO_ROW, 0))
        assert linear_zero != neutral, (
            "a linear map over an asymmetric domain must NOT put the neutral at zero "
            f"(got {linear_zero}, neutral {neutral}) — if these matched, the demo would "
            "prove nothing"
        )
        print(f"  ok: the linear map puts zero at {linear_zero}, not the neutral")

        # --- the two wings are distinguishable, and signed the right way. ---
        # Read every value ONCE, then judge the EXTREMES of each wing: a cell
        # just past the baseline is legitimately near-neutral (that is what a
        # diverging ramp is for), so the hue claim is only meaningful at the
        # ends.
        values = {
            (r, c): float(at(tf, "value_at", (r, c)))
            for r in range(ROWS)
            for c in range(COLS)
        }
        below = [rc for rc, v in values.items() if v < -1.0]
        above = [rc for rc, v in values.items() if v > 1.0]
        assert below, "the grid must exercise the below-baseline wing"
        assert above, "the grid must exercise the above-baseline wing"
        coldest = min(values, key=lambda rc: values[rc])
        hottest = max(values, key=lambda rc: values[rc])
        below_hex = at(tf, "color_at", coldest)
        above_hex = at(tf, "color_at", hottest)
        assert below_hex != neutral and above_hex != neutral, "off-target cells are not neutral"
        assert below_hex != above_hex, "the two wings are different colours"

        def rgb(h: str) -> tuple[int, int, int]:
            return (int(h[1:3], 16), int(h[3:5], 16), int(h[5:7], 16))

        b_r, _, b_b = rgb(below_hex)
        a_r, _, a_b = rgb(above_hex)
        assert b_b > b_r, (
            f"the coldest cell {coldest} (value {values[coldest]}) is the blue end: {below_hex}"
        )
        assert a_r > a_b, (
            f"the hottest cell {hottest} (value {values[hottest]}) is the warm end: {above_hex}"
        )
        print(
            f"  ok: coldest {values[coldest]:.1f}={below_hex} (blue) / "
            f"hottest {values[hottest]:.1f}={above_hex} (warm), {len(below)}+{len(above)} off-target cells"
        )
        # Saturation ranks with magnitude: a cell nearer the baseline is nearer
        # the neutral than the extreme is (the ramp encodes HOW FAR, not just
        # which side).
        mild = min(above, key=lambda rc: values[rc])
        mild_hex = at(tf, "color_at", mild)
        assert abs(rgb(mild_hex)[0] - rgb(neutral)[0]) < abs(a_r - rgb(neutral)[0]), (
            f"a mild deviation {mild_hex} must sit nearer the neutral than the extreme {above_hex}"
        )
        print(f"  ok: mild {values[mild]:.1f}={mild_hex} sits between neutral and the extreme")

        # --- ACCESSIBILITY as data: the worst cell still clears WCAG 4.5:1. ---
        worst = float(tf.query(f"{EXT}/min_contrast"))
        assert worst >= 4.5, f"worst cell contrast {worst} must clear the 4.5:1 small-text floor"
        # With MARGIN: a near-black ink cleared this ramp by 0.0005, which is a
        # coincidence and not a design (the demo pins pure black for that
        # reason). Asserting the margin is what keeps the claim honest.
        assert worst >= 5.0, f"worst cell contrast {worst} must clear the floor with margin"
        print(f"  ok: worst cell contrast {worst:.2f}:1 clears WCAG 4.5")
        # And it is a real per-cell figure. Rather than trusting the crate's own
        # arithmetic, recompute WCAG here INDEPENDENTLY (sRGB EOTF + BT.709
        # luminance + the (L+0.05) ratio) and cross-validate: a bug in the Rust
        # implementation cannot hide behind a test that calls the same code.
        def srgb_to_linear(channel: int) -> float:
            s = channel / 255.0
            return s / 12.92 if s <= 0.04045 else ((s + 0.055) / 1.055) ** 2.4

        def luminance(h: str) -> float:
            r, g, b = rgb(h)
            return (
                0.2126 * srgb_to_linear(r)
                + 0.7152 * srgb_to_linear(g)
                + 0.0722 * srgb_to_linear(b)
            )

        def ratio(bg: str, ink: str) -> float:
            la, lb = luminance(bg), luminance(ink)
            return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)

        probes = [(0, 0), (ZERO_ROW, 3), (ROWS - 1, COLS - 1), coldest, hottest]
        inks = set()
        computed_worst = float("inf")
        for probe in probes:
            bg = at(tf, "color_at", probe)
            ink = at(tf, "ink_at", probe)
            inks.add(ink)
            published = float(at(tf, "contrast_at", probe))
            mine = ratio(bg, ink)
            assert abs(published - mine) < 0.01, (
                f"cell {probe}: the crate says {published:.3f}:1, an independent WCAG "
                f"computation says {mine:.3f}:1 for ink {ink} on {bg}"
            )
            # The chosen ink must beat the OTHER candidate — the contract of a
            # computed (not thresholded) ink choice.
            other = "#ffffff" if ink == "#000000" else "#000000"
            assert mine >= ratio(bg, other) - 1e-9, (
                f"cell {probe}: chosen ink {ink} ({mine:.2f}) loses to {other} "
                f"({ratio(bg, other):.2f}) on {bg}"
            )
            assert published >= worst - 1e-6, f"cell {probe} ratio {published} < published min {worst}"
            computed_worst = min(computed_worst, mine)
        print(f"  ok: WCAG cross-validated on {len(probes)} cells (independent recompute)")
        # This ramp resolves to ONE ink everywhere — which is the finding, not a
        # defect: its endpoints are mid-luminance, so a fixed "light ink on the
        # dark half" rule would have put unreadable pale text on the saturated
        # cells. The choice being COMPUTED is what makes that come out right.
        for ink in inks:
            assert ink in ("#000000", "#ffffff"), f"ink came from the pinned pair: {ink}"
        print(f"  ok: ink is the computed winner everywhere ({sorted(inks)})")

        # --- the paint agrees with the model (§2 #7): cells are laid out. ---
        first = find_by_tag(snap, cell_tag(0, 0))["rect"]
        right = find_by_tag(snap, cell_tag(0, 1))["rect"]
        down = find_by_tag(snap, cell_tag(1, 0))["rect"]
        assert_eq(first["w"], CELL_W, "cell width matches the model")
        assert_eq(first["h"], CELL_H, "cell height matches the model")
        assert_eq(right["x"] - first["x"], CELL_W + CELL_GAP, "columns abut with the gap")
        assert_eq(down["y"] - first["y"], CELL_H + CELL_GAP, "rows abut with the gap")
        assert_eq(right["y"], first["y"], "a row shares one baseline")
        last = find_by_tag(snap, cell_tag(ROWS - 1, COLS - 1))["rect"]
        assert last["x"] + last["w"] <= WIN[0], "the grid fits the window in x"
        assert last["y"] + last["h"] <= WIN[1], "the grid fits the window in y"
        print(f"  ok: {ROWS}x{COLS} cells laid out, last at {last}")

        # --- the readout names the domain + the contrast floor. ---
        readout = find_by_tag(snap, "dev.readout")
        assert readout is not None, "the readout line is painted"
        text = readout.get("content", "")
        assert "asymmetric" in text, f"the readout names the domain shape: {text!r}"
        assert "contrast" in text, f"the readout names the contrast floor: {text!r}"
        print(f"  ok: readout {text!r}")

        # --- wire contract: the cell oracles reject what they cannot answer. ---
        for arg, why in [
            ("99,0", "row out of range"),
            ("0,99", "column out of range"),
            ("nope", "malformed pair"),
            ("", "empty argument"),
        ]:
            try:
                tf.invoke(f"{EXT}/value_at", arg)
                raise AssertionError(f"{why} must be rejected")
            except RpcError as exc:
                print(f"  ok: {why} rejected ({exc.message!r})")

        # --- and the model is read-only: an intervene on a projection fails. ---
        try:
            tf.intervene(f"{EXT}/min_contrast", 21.0)
            raise AssertionError("min_contrast is a projection and must be read-only")
        except RpcError as exc:
            print(f"  ok: intervene on a read-only projection rejected ({exc.message!r})")

        # --- recovery: a valid oracle call after the rejects still answers. ---
        assert_eq(
            at(tf, "color_at", (ZERO_ROW, 1)),
            neutral,
            "a real query recovers after the rejects",
        )


if __name__ == "__main__":
    sys.exit(run_demo("r1436_deviation_grid", body))
