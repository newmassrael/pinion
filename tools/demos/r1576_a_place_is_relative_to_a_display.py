#!/usr/bin/env python3
"""R1576 §5.16 §5.41 §2 #7 — a place is stated relative to a DISPLAY.

`hello-displays` declares two windows. The `panel` window's position is not a
desktop coordinate: it is "48 logical pixels into *that* monitor", and which
monitor is a name a layout preset can hold. When that monitor is not attached,
the substitution onto the fallback is **reported**, so a restored layout that
had to move says so instead of quietly opening where no pixel is.

Before this round pinion had no notion of a monitor at all — a census found
zero references to `available_monitors` in the whole tree — so `WindowSpec`'s
`position` was a coordinate in a space the framework could not describe, and
"is this window even on screen?" was unaskable.

What this script checks, and why each check discriminates:

* **The desk is the same fact on both sides.** `scene/displays` (the framework
  reading the window system) and the binding's own `query display_ids` (a
  binding reading `use_displays()`) are compared against each other. One
  implementation, two readers: if the handle the binding holds ever stopped
  tracking the surface's stamp, these would diverge.
* **The published numbers are consistent with themselves.** `covered_px`
  against the bounding box's area, `gap_free` against the difference, each
  display's `logical_size` against its own bounds and scale, and — the sum
  check the analyzer-class discipline asks for — `visible_px + offscreen_px ==
  total_px` on every probe.
* **PAST Qt 6.11 (1): a rectangle can be resolved before it is used.** Every Qt
  screen query takes a *point* (`screenAt`, `virtualSiblingAt`); there is no
  rectangle-level question in the API, which is why each Qt application that
  restores a geometry hand-rolls its own clamp. The `probe` parameter answers
  with the home display, every display touched, the pixels that are on a
  display, and where the window would have to move to be wholly visible.
* **PAST Qt 6.11 (2): the substitution is named.** A preset asks for a display
  that is not attached. `scene/windows` reports `anchored.kind ==
  "substituted"` with **both** the declared id and the one used.
  `QWidget::restoreGeometry` answers a bare `bool`.
* **PAST Qt 6.11 (3): a display has an address at all.** `QScreen` has no id
  accessor — `name()` is platform text with no uniqueness guarantee — so a Qt
  preset cannot name a monitor. The ids here are unique by construction, which
  the script asserts over whatever the host actually has.
* **PAST Qt 6.11 (4): absence is stated.** A display whose refresh rate the
  platform did not report answers `null`, never a plausible `0`.
  `QScreen::refreshRate()` returns `qreal`.
* **PAST Qt 6.11 (5): the desk reaches the wire.** `QGuiApplication::screens()`
  is in-process C++; no external client can ask a running Qt application what
  it is displaying on.
* **The declaration and the report cannot drift.** The panel's placement is
  changed by writing its spec, and every read — the binding's, the wire's, and
  the window's own painted line — is re-derived from that one declaration.
* **The refusals name what they refused.** A malformed `probe`, an unknown
  preset, and a write to a derived path each answer distinctly.

Everything here holds on **any** desk, including the single-monitor one CI has:
the assertions are relations between published facts, never absolute geometry
of the host's own monitors. The multi-monitor arithmetic is exercised
exhaustively in `pinion_core::display`'s unit tests, where an arrangement is an
argument rather than hardware.

Run from the workspace root:
    cargo build -p hello-displays --release
    python3 tools/demos/r1576_a_place_is_relative_to_a_display.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_action_refused,
    assert_eq,
    assert_rpc_error,
    call,
    rpc_error_data,
    run_demo,
)

#: Mirrored from the binding rather than imported — a demo reading its expected
#: answers out of the code under test cannot catch that code changing.
PANEL_INSET = (48, 48)
ABSENT_DISPLAY = "external-4k"
PRESETS = ["primary", "external", "absolute", "far", "unplaced"]
PANEL_SIZE = (320, 220)


def q(tf: RpcSubprocess, path: str):
    """One `query` against the primary External."""
    return tf.query(f"/external/{path}")


def displays(tf: RpcSubprocess, **params) -> dict:
    return call(tf, "scene/displays", params or {})


def windows(tf: RpcSubprocess) -> dict[str, dict]:
    return {w["id"]: w for w in call(tf, "scene/windows")["windows"]}


def rect_area(rect: dict) -> int:
    return rect["w"] * rect["h"]


def run(tf: RpcSubprocess) -> None:
    # ---- 0. the method exists and is discoverable ----------------------
    catalogue = {m["name"] for m in call(tf, "rpc/methods")["methods"]}
    assert "scene/displays" in catalogue, (
        "the desk must be DISCOVERABLE, not only callable — an agent finds the "
        "surface through rpc/methods"
    )

    desk = displays(tf)
    ids = [d["id"] for d in desk["displays"]]
    print(f"[demo] the desk: {len(ids)} display(s) {ids}, gap_free={desk['gap_free']}")

    # ---- 1. invariants that hold on ANY host ---------------------------
    assert_eq(len(set(ids)), len(ids), "a display id is unique by construction")
    assert all(i for i in ids), "every display has a non-empty id"
    primaries = [d for d in desk["displays"] if d["primary"]]
    assert len(primaries) <= 1, f"at most one display is primary, got {len(primaries)}"
    assert_eq(
        desk["primary"],
        primaries[0]["id"] if primaries else None,
        "the reported primary is the one flagged primary",
    )
    if ids:
        assert desk["fallback"] in ids, "the fallback is a display that exists"
        assert_eq(
            desk["fallback"],
            desk["primary"] or ids[0],
            "the fallback is the primary, else the first enumerated",
        )
    else:
        assert_eq(desk["fallback"], None, "a headless desk has no fallback")

    # The union arithmetic, checked against the bounding box it is compared to.
    if desk["bounding_box"] is None:
        assert_eq(desk["covered_px"], 0, "no displays cover no pixels")
        assert desk["gap_free"], "an empty union fills an empty bounding box"
    else:
        bb_area = rect_area(desk["bounding_box"])
        assert desk["covered_px"] <= bb_area, (
            f"covered {desk['covered_px']} exceeds its own bounding box {bb_area}"
        )
        assert_eq(
            desk["gap_free"],
            desk["covered_px"] == bb_area,
            "gap_free IS 'the union fills the bounding box' — the two must agree, "
            "and this is the distinction Qt's virtualGeometry() cannot express",
        )
        # Every display is inside the bounding box, and contributes to it.
        for d in desk["displays"]:
            b, bb = d["bounds"], desk["bounding_box"]
            assert bb["x"] <= b["x"] and bb["y"] <= b["y"], f"{d['id']} starts inside the bound"
            assert b["x"] + b["w"] <= bb["x"] + bb["w"], f"{d['id']} ends inside the bound"
            assert b["y"] + b["h"] <= bb["y"] + bb["h"], f"{d['id']} ends inside the bound"

    for d in desk["displays"]:
        assert d["scale"] > 0, f"{d['id']} reports a usable scale"
        # PAST Qt: absence is stated. `QScreen::refreshRate()` is a `qreal`, so
        # "unknown" is indistinguishable from a real 0 there.
        assert d["refresh_mhz"] is None or d["refresh_mhz"] > 0, (
            f"{d['id']} reports a refresh rate or null, never a plausible 0"
        )
        assert abs(d["logical_size"]["w"] * d["scale"] - d["bounds"]["w"]) < 1e-6, (
            f"{d['id']}'s logical width is its physical width over its OWN scale"
        )
        assert abs(d["logical_size"]["h"] * d["scale"] - d["bounds"]["h"]) < 1e-6, (
            f"{d['id']}'s logical height is its physical height over its OWN scale"
        )

    # ---- 2. one desk, two readers --------------------------------------
    # The framework read the window system; the binding read `use_displays()`.
    # If the handle ever stopped tracking the surface's stamp, these diverge.
    assert_eq(int(q(tf, "display_count")), len(ids), "the binding counts the same displays")
    assert_eq(
        [s for s in str(q(tf, "display_ids")).split(",") if s],
        ids,
        "★the binding NAMES the same displays, in the same order — one desk, two readers",
    )
    assert_eq(q(tf, "primary_id"), desk["primary"] or "", "and agrees on the primary")
    assert_eq(q(tf, "fallback_id"), desk["fallback"] or "", "and on the fallback")
    assert_eq(str(q(tf, "gap_free")), str(desk["gap_free"]).lower(), "and on the holes")
    assert_eq(int(q(tf, "covered_px")), desk["covered_px"], "and on the covered pixels")

    # ---- 3. the derived answers are ABSENT unless asked for -------------
    for key in ("at", "placement", "anchored"):
        assert key not in desk, (
            f"{key} must be absent when nothing asked — a null would read as an answer"
        )

    # ---- 4. PAST Qt: resolving a RECTANGLE ------------------------------
    if ids:
        home = desk["displays"][0]
        b = home["bounds"]
        # (a) wholly inside one display.
        inside = displays(tf, probe={"x": b["x"], "y": b["y"], "w": 10, "h": 10})["placement"]
        assert_eq(inside["home"], home["id"], "a window in a display's corner is on it")
        assert inside["fully_visible"], "and wholly visible"
        assert_eq(inside["offscreen_px"], 0, "with nothing lost")
        assert_eq(inside["visible_px"], 100, "and its pixels counted")
        assert_eq(
            inside["suggestion"],
            [b["x"], b["y"]],
            "the suggestion is where it already is — present on success too, so a "
            "caller can check its own arithmetic against it",
        )
        assert_eq(
            [c["id"] for c in inside["covering"]],
            [home["id"]],
            "exactly one display is touched",
        )

    # (b) a window where no pixel is — the unplugged-monitor case.
    far = displays(tf, probe={"x": 900_000, "y": 900_000, "w": 800, "h": 600})["placement"]
    assert_eq(far["home"], None, "★a window on no display says so, rather than guessing one")
    assert_eq(far["covering"], [], "and touches nothing")
    assert_eq(far["visible_px"], 0, "and none of it is visible")
    assert_eq(far["total_px"], 800 * 600, "though it is a real size")
    assert not far["fully_visible"]
    assert_eq(far["visible_fraction"], 0.0)
    if ids:
        assert far["suggestion"] is not None, (
            "★and the framework says where it would have to move — the answer every "
            "Qt application hand-rolls because the API has no rectangle question"
        )
        back = displays(
            tf,
            probe={
                "x": far["suggestion"][0],
                "y": far["suggestion"][1],
                "w": 800,
                "h": 600,
            },
        )["placement"]
        assert back["fully_visible"], (
            "★and taking the suggestion WORKS — the answer is checked by feeding it "
            "back through the same method, not merely by being non-null"
        )

    # (c) the sum check, on every probe seen so far.
    probes = [("far", far)]
    if ids:
        probes.append(("inside", inside))
    for label, p in probes:
        assert_eq(
            p["visible_px"] + p["offscreen_px"],
            p["total_px"],
            f"{label}: the published pixel counts must add up",
        )
        summed = sum(c["px"] for c in p["covering"])
        assert summed >= p["visible_px"], (
            f"{label}: per-display shares over-count an overlap; the union never does"
        )

    # ---- 5. the point question, and the absent-vs-null distinction ------
    if ids:
        b = desk["displays"][0]["bounds"]
        on = displays(tf, at=[b["x"], b["y"]])
        assert_eq(on["at"]["display"], desk["displays"][0]["id"], "a point on a display")
    nowhere = displays(tf, at=[900_000, 900_000])
    assert "at" in nowhere, "asked, so the key is present"
    assert_eq(
        nowhere["at"]["display"],
        None,
        "★and the answer is null INSIDE it — 'asked and it is nowhere' stays "
        "distinct from 'did not ask'",
    )

    # ---- 6. the panel boots UNPLACED ------------------------------------
    panel = windows(tf)["panel"]
    assert_eq(panel["position"], None, "the panel boots WM-placed")
    assert_eq(panel["display"], None, "with no display declared")
    assert_eq(
        panel["anchored"],
        None,
        "★and `anchored` is null — 'declares no place' is a different fact from "
        "'declares a place that resolves to nothing'",
    )

    # ---- 7. PAST Qt: a place relative to a NAMED display -----------------
    if ids:
        summary = tf.invoke("/external/apply", "primary")
        print(f"[demo] preset primary -> {summary}")
        panel = windows(tf)["panel"]
        assert_eq(panel["display"], desk["primary"] or ids[0], "the panel names a display")
        assert_eq(panel["position"], list(PANEL_INSET), "at a LOGICAL offset into it")
        anchored = panel["anchored"]
        assert_eq(anchored["kind"], "on_declared", "and the display is attached")
        assert_eq(anchored["declared"], anchored["display"], "so nothing was substituted")
        d = next(x for x in desk["displays"] if x["id"] == anchored["display"])
        assert_eq(
            anchored["at"],
            [
                d["bounds"]["x"] + round(PANEL_INSET[0] * d["scale"]),
                d["bounds"]["y"] + round(PANEL_INSET[1] * d["scale"]),
            ],
            "★the resolved point is that display's corner plus the offset SCALED by "
            "that display's own factor — which is what makes one preset mean one "
            "visible distance on monitors of different densities",
        )
        landed = displays(
            tf,
            probe={
                "x": anchored["at"][0],
                "y": anchored["at"][1],
                "w": PANEL_SIZE[0],
                "h": PANEL_SIZE[1],
            },
        )["placement"]
        assert_eq(landed["home"], anchored["display"], "and the window lands on that display")

    # ---- 8. PAST Qt: the substitution is NAMED ---------------------------
    summary = tf.invoke("/external/apply", "external")
    print(f"[demo] preset external -> {summary}")
    panel = windows(tf)["panel"]
    assert_eq(
        panel["display"],
        ABSENT_DISPLAY,
        "the DECLARED display reads back verbatim, including one that is not here — "
        "a preset that silently rewrote itself could never be corrected",
    )
    anchored = panel["anchored"]
    if ids:
        assert_eq(anchored["kind"], "substituted", "★the display is gone, and the answer says so")
        assert_eq(anchored["declared"], ABSENT_DISPLAY, "★naming what was asked for")
        assert_eq(anchored["display"], desk["fallback"], "★and what was used instead")
        assert anchored["display"] != anchored["declared"], "the two differ, which is the point"
        reachable = displays(
            tf,
            probe={
                "x": anchored["at"][0],
                "y": anchored["at"][1],
                "w": PANEL_SIZE[0],
                "h": PANEL_SIZE[1],
            },
        )["placement"]
        assert reachable["fully_visible"], (
            "★and the substituted window is somewhere a person can actually reach — "
            "which is the whole reason to substitute rather than obey"
        )
    else:
        assert_eq(anchored["kind"], "no_display", "a headless desk has no place at all")
        assert_eq(anchored["at"], None)

    # ---- 9. an ABSOLUTE placement still means what it always did ---------
    tf.invoke("/external/apply", "absolute")
    panel = windows(tf)["panel"]
    assert_eq(panel["display"], None, "an absolute preset CLEARS the display it replaced")
    assert_eq(panel["position"], [120, 120], "and keeps the desktop coordinate exactly")

    # ---- 10. a window declared where no pixel is -------------------------
    tf.invoke("/external/apply", "far")
    panel = windows(tf)["panel"]
    assert_eq(panel["position"], [9000, 40], "the declaration is honoured verbatim")
    anchored = panel["anchored"]
    if ids:
        bb = desk["bounding_box"]
        beyond = bb["x"] + bb["w"] <= 9000
        assert_eq(
            anchored["kind"],
            "no_display" if beyond else "on_declared",
            "★an absolute position off every display reports NO display rather than "
            "naming a plausible one",
        )

    # ---- 11. back to unplaced, and the round trip closes ------------------
    tf.invoke("/external/apply", "unplaced")
    panel = windows(tf)["panel"]
    assert_eq(panel["position"], None, "the placement is cleared, not merely overwritten")
    assert_eq(panel["display"], None)
    assert_eq(panel["anchored"], None, "and the window is WM-placed again, as it booted")

    # ---- 12. the binding resolves through the SAME function ---------------
    answer = str(tf.invoke("/external/resolve", "900000,900000,800,600"))
    assert "home=none" in answer, f"the binding's own resolve agrees: {answer}"
    assert "visible=0" in answer, answer
    assert f"total={800 * 600}" in answer, answer

    # ---- 13. refusals name what they refused ------------------------------
    data = rpc_error_data(
        lambda: call(tf, "scene/displays", {"probe": {"x": 0, "y": 0, "w": 10}}),
        label="a probe missing an extent",
    )
    assert data.startswith("MalformedDisplayAsk"), (
        f"the refusal carries a MATCHABLE word (-32602 publishes a closed data "
        f"vocabulary), got {data!r}"
    )
    assert "probe.h" in data, f"and names the offending parameter path, got {data!r}"
    anchor_data = rpc_error_data(
        lambda: call(tf, "scene/displays", {"anchor": {"offset": [0, 0]}}),
        label="an anchor with no display",
    )
    assert "anchor.display" in anchor_data, (
        f"and names the parameter that was missing, got {anchor_data!r}"
    )
    assert_action_refused(
        lambda: tf.invoke("/external/apply", "no-such-preset"),
        saying="is not a preset",
    )
    assert_rpc_error(
        lambda: tf.intervene("/external/display_count", 3),
        data="ReadOnly",
    )

    # ---- 14. the presets are published as data ----------------------------
    assert_eq(
        [s for s in str(q(tf, "preset_names")).split(",") if s],
        PRESETS,
        "a layout preset is DATA a client can enumerate — Qt's saveGeometry is an "
        "opaque QByteArray that cannot be read, diffed or edited",
    )

    # ---- 15. the desk reaches assistive technology ------------------------
    access = call(tf, "scene/access")
    text = str(access)
    assert "display(s)" in text, "the accessible value names how many displays"
    assert "gap-free" in text, "and whether the desk has holes"
    print("[demo] the desk, the placement and the substitution all read off the wire")


def body() -> None:
    with RpcSubprocess("hello-displays", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("r1576 a place is relative to a display", body)
