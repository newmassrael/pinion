#!/usr/bin/env python3
"""R1714 §5.16 §5.32 §5.12 §2 #3 §2 #7 — **a window smaller than its layout
moves over it instead of cutting into it**, on the analysis tool's own screens.

# What this exists for

Every screen here served the space below its layout minimum the same way, by
construction rather than by decision: `layout_size` clamps the layout at the
comfortable size and the window clips whatever sticks out, with no range
anywhere to move it. R1712 gave that band a declaration and R1713 measured what
it costs — and the measurement is what showed the ceiling on the whole idea. The
node lab's band bottomed out **24 pixels** down, at 1601, because one pixel
lower five glyphs stop being reachable by anything; and 1601 misses by one pixel
the 1600-wide display R1689 wrote a loss against. The last pixel could not be
bought, because below the comfortable size the layout stops reflowing and what
the window cuts is simply gone.

R1714 makes the window a **viewport onto the layout**, declared by the policy
(`ShrinkPolicy::panning`) and built by the framework, so nothing is out of reach
at any size. The floor stops being a measurement and becomes a decision — and
the answer to "what would it take to see this" stops being one offset and
becomes the **chain**, because a panning window makes every clip chain on these
screens two levels deep, and the single offset it used to publish was then not
merely incomplete but wrong.

# What it asserts

* **A** — the declaration reaches the wire: the panning screen publishes
  `recourse: pan`, an empty `gives_up`, and the floor it decided; the two
  clipping screens still publish `clip`. One reader, three screens, and the word
  is what tells them apart.
* **B** — ★★★★★ the headline, driven rather than declared: across the whole band,
  at every width down to the floor, **nothing is `lost` and nothing is
  `clipped`**. That is the promise a pan makes, and it is checked at the
  boundary the search finds rather than at a number written here.
* **C** — ★★★★★ the chain. At the width the previous round shipped as a floor,
  the marks it lost are `scrollable` — and the recipe **names the window's pan**,
  outermost first. The recipe is then PERFORMED through the wire and the marks
  are on screen afterwards, which is what makes it a recipe and not an opinion.
* **D** — ★★★ R1689's payoff, finally taken: the floor is at or below 1600, so a
  1600-wide display holds this screen — and the window really goes there.
* **E** — ★★★★ the press follows the pan. `scene/pointer_target` asks the screen,
  for every painted rectangle, what a press inside it addresses; measured with
  the pan ignored it fell to 1 of 57 with 26 unreachable. Asserted at the floor
  AND after panning, because only the second one moved.
* **F** — ★★ §2 #3: every hypothetical above is answered without the window
  moving, and the pan a screen does not declare is not built (the two clipping
  screens have no pan node at any size).
* **G** — ★ the pixels: a photograph at the floor and one panned, and the band
  the window had cut is drawn in the second.
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    Png,
    RpcSubprocess,
    assert_eq,
    abs_rects_of,
    declared_and_painted,
    design_size,
    png_pixel,
    read_png_rgba8,
    resize_and_settle,
    run_demo,
)

#: The three screens, in the order the specification names them.
SCREENS = [
    ("node lab", "hello-node-lab"),
    ("capture viewer", "hello-packet-view"),
    ("dashboard", "hello-analyzer-shell"),
]

#: The one screen that pans, and the width the round before this one shipped as
#: its floor. Written here so this file asserts the DECISION rather than echoing
#: whatever the binding currently says.
PANNING = "hello-node-lab"
PREVIOUS_FLOOR = 1601

#: R1689's requirement, in pixels: a display this wide must hold the screen.
#: The number a real loss was written against, and the one the band exists for.
DISPLAY = 1600

#: The name the framework gives the window's own pan. A constant here because a
#: recipe that named something else would still be a recipe — and would not be
#: this one.
PAN = "window.pan"

CHECKS: list[str] = []


def ok(what: str, condition: bool) -> None:
    assert condition, f"FAILED: {what}"
    CHECKS.append(what)


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def reach_at(app: RpcSubprocess, size: tuple[int, int]) -> dict:
    resp = app.request(
        "scene/scroll_reach", {"at": {"width": size[0], "height": size[1]}}
    )
    assert resp is not None and isinstance(resp.result, dict), "scroll_reach answers"
    return resp.result


def floor_report(app: RpcSubprocess) -> dict:
    resp = app.request("scene/size_floor")
    assert resp is not None and isinstance(resp.result, dict)
    return resp.result


def rows(reach: dict, verdict: str) -> list[dict]:
    return [row for row in reach["out_of_sight"] if row["reach"] == verdict]


# ── A: the declaration on the wire ──────────────────────────────────────────


def a_the_recourse_is_published(app: RpcSubprocess, name: str, example: str) -> dict:
    report = floor_report(app)
    concession = report["concession"]
    pans = example == PANNING
    assert_eq(
        concession["recourse"],
        "pan" if pans else "clip",
        f"A/{name}: the screen says how its band is served",
    )
    if pans:
        ok(
            f"A/{name}: and a pan names nothing it gives up ({concession['gives_up']}), "
            f"because it gives nothing up",
            not concession["gives_up"],
        )
        assert_eq(
            report["verdict"],
            "panned",
            f"A/{name}: the floor reads as a decision made by moving, not as a "
            f"clipping concession and not as a bare `roomier`",
        )
    else:
        ok(
            f"A/{name}: a clipping screen still reads `clip` and keeps its list "
            f"({concession['gives_up']})",
            concession["verdict"] in ("honoured", "stale"),
        )
    ok(
        f"A/{name}: the declaration is the one the window was built from "
        f"(declaration_split={concession['declaration_split']})",
        not concession["declaration_split"],
    )
    return concession


# ── B: the promise, across the band ─────────────────────────────────────────


def b_nothing_is_out_of_reach_anywhere_in_the_band(
    app: RpcSubprocess, name: str, concession: dict
) -> None:
    """★★★★★ Driven across the band, not asserted at one size."""
    floor = (concession["floor"]["width"], concession["floor"]["height"])
    ceiling = (concession["comfortable"]["width"], concession["comfortable"]["height"])
    band = ceiling[0] - floor[0]
    ok(f"B/{name}: there is a band to check ({band} pixels wide)", band > 0)
    # Every width in the band, sampled at every eighth pixel plus both ends —
    # the ends are where an off-by-one lives and the middle is where a pane
    # stops being narrowed and starts being gone.
    widths = sorted({floor[0], ceiling[0], *range(floor[0], ceiling[0], max(1, band // 8))})
    bad = []
    for w in widths:
        reach = reach_at(app, (w, floor[1]))
        if reach["lost"] or reach["clipped"]:
            bad.append((w, reach["lost"], reach["clipped"]))
    ok(
        f"B/{name}: across {len(widths)} widths from {floor[0]} to {ceiling[0]}, "
        f"nothing is lost and nothing is clipped",
        not bad,
    )
    # ★★ And below the floor too: the pan does not stop working at the number
    # the screen decided to stop at, which is what makes the floor a decision.
    below = reach_at(app, (floor[0] // 2, floor[1]))
    ok(
        f"B/{name}: and at half the floor ({floor[0] // 2}) it is still all "
        f"reachable ({below['scrollable']} scrollable, {below['lost']} lost)",
        below["lost"] == 0 and below["clipped"] == 0 and below["scrollable"] > 0,
    )
    # ★★ The comparison is not vacuous: there IS something off screen to judge.
    at_floor = reach_at(app, floor)
    ok(
        f"B/{name}: and the band is not empty of marks to lose "
        f"({len(at_floor['out_of_sight'])} off screen of {at_floor['marks']})",
        len(at_floor["out_of_sight"]) > 20,
    )
    # ★★★★ The arithmetic everything above rests on: the pan's range is exactly
    # what the window is short by. A range one pixel small is a mark lost at the
    # far edge, which `lost` would catch only once something is painted there —
    # so it is asserted directly, at every width, rather than waited for.
    wrong = []
    for w in widths:
        reach = reach_at(app, (w, floor[1]))
        ranges = {
            row["viewport"]["max_x"]
            for row in reach["out_of_sight"]
            if row["viewport"]["name"] == PAN
        }
        if ranges and ranges != {ceiling[0] - w}:
            wrong.append((w, sorted(ranges), ceiling[0] - w))
    ok(
        f"B/{name}: and the pan's range is exactly the shortfall at every width "
        f"({len(widths)} checked)",
        not wrong,
    )
    # ★★★ The pan exists exactly when there is something to pan over, and this
    # is where that is checked: at the comfortable size the window shows the
    # whole layout, so there is no pan and its name resolves to nothing. A pan
    # with no range is not a pan — the argument `Fault::BandNamesNothing` makes
    # about a band that costs nothing.
    try:
        app.request("scene/scroll", {"path": PAN, "to": {"x": 0, "y": 0}})
        absent = False
    except Exception:  # noqa: BLE001 — the refusal is the answer
        absent = True
    ok(
        f"B/{name}: at the comfortable size there is no pan to drive, because "
        f"there is nothing to pan over",
        absent,
    )


# ── C: the chain, named and then performed ──────────────────────────────────


def c_the_recipe_names_the_pan_and_works(app: RpcSubprocess, name: str) -> None:
    """★★★★★ The repair, checked by doing what it says."""
    at = (PREVIOUS_FLOOR - 1, 360)
    reach = reach_at(app, at)
    ok(
        f"C/{name}: at {at[0]} — one pixel under the floor the round before this "
        f"one shipped — nothing is lost ({reach['lost']})",
        reach["lost"] == 0,
    )
    # The marks that round lost are the `×` glyphs inside the inspector's row
    # actions. They carry no tag, so they are found by their content.
    glyphs = [r for r in reach["out_of_sight"] if r["content"] == "×"]
    ok(
        f"C/{name}: the marks that were lost there are reported, by content "
        f"({len(glyphs)} of them)",
        len(glyphs) >= 5,
    )
    deep = [g for g in glyphs if len(g["moves"]) > 1]
    ok(
        f"C/{name}: and at least one needs TWO viewports moved — the recipe a "
        f"single offset could not write ({len(deep)} of {len(glyphs)})",
        bool(deep),
    )
    for g in glyphs:
        assert g["moves"], f"C/{name}: a scrollable mark with an empty recipe: {g}"
        assert_eq(
            g["moves"][0]["viewport"],
            PAN,
            f"C/{name}: the recipe starts at the outermost viewport",
        )
        # ★ A subsequence of the chain, outermost first — a viewport already at
        # the offset it needs is deliberately absent, because the list is what
        # must CHANGE. Two of these five glyphs need only the window moved.
        chain = [PAN, g["viewport"]["name"]]
        named = [m["viewport"] for m in g["moves"]]
        assert named == [step for step in chain if step in named], (
            f"C/{name}: the recipe is not the chain in order: {named} of {chain}"
        )
    ok(
        f"C/{name}: every recipe runs outermost first, naming only viewports on "
        f"the mark's own chain and only where the offset has to change "
        f"({sorted({len(g['moves']) for g in glyphs})} step(s) seen)",
        True,
    )
    # ★★★★★ Now PERFORM one, in the live window, and look.
    target = deep[0]
    resize_and_settle(app, at)
    assert_eq(design_size(app), at, f"C/{name}: the window took that width")
    live = app.request("scene/scroll_reach")
    assert live is not None and isinstance(live.result, dict)
    live_glyphs = [r for r in live.result["out_of_sight"] if r["content"] == "×"]
    assert live_glyphs, f"C/{name}: the glyphs are off screen in the live window"
    # ★★ And below the comfortable size the pan IS there to drive — the other
    # half of the property section B checks at the top of the band.
    ok(
        f"C/{name}: the pan is a path the wire accepts once there is something "
        f"to pan over",
        app.request("scene/scroll", {"path": PAN, "to": {"x": 0, "y": 0}}) is not None,
    )
    recipe = live_glyphs[0]["moves"]
    for move in recipe:
        resp = app.request(
            "scene/scroll",
            {"path": move["viewport"], "to": {"x": move["to_x"], "y": move["to_y"]}},
        )
        assert resp is not None, f"C/{name}: {move['viewport']} refused {move}"
    app.tick(0.016)
    after = app.request("scene/scroll_reach")
    assert after is not None and isinstance(after.result, dict)
    still = [r for r in after.result["out_of_sight"] if r["content"] == "×"]
    ok(
        f"C/{name}: performing the recipe {[(m['viewport'], m['to_x'], m['to_y']) for m in recipe]} "
        f"brought the mark on screen ({len(live_glyphs)} off screen before, "
        f"{len(still)} after)",
        len(still) < len(live_glyphs),
    )
    for move in recipe:
        app.request("scene/scroll", {"path": move["viewport"], "to": {"x": 0, "y": 0}})
    app.tick(0.016)
    _ = target


# ── D: the payoff R1689 wrote down ──────────────────────────────────────────


def d_a_1600_wide_display_holds_the_screen(
    app: RpcSubprocess, name: str, concession: dict
) -> None:
    floor = (concession["floor"]["width"], concession["floor"]["height"])
    ok(
        f"D/{name}: the floor is {floor[0]}, which a {DISPLAY}-pixel display "
        f"holds with {DISPLAY - floor[0]} to spare — the loss R1689 wrote down, "
        f"paid",
        floor[0] <= DISPLAY,
    )
    granted = resize_and_settle(app, (DISPLAY, floor[1]))
    assert_eq(
        design_size(app),
        (DISPLAY, floor[1]),
        f"D/{name}: and the window really goes there",
    )
    live = app.request("scene/scroll_reach")
    assert live is not None and isinstance(live.result, dict)
    assert_eq(
        live.result["lost"],
        0,
        f"D/{name}: with nothing out of reach on a display that size",
    )
    _ = granted


# ── E: the press follows the pan ────────────────────────────────────────────


def e_a_press_lands_after_the_pan(
    app: RpcSubprocess, name: str, concession: dict
) -> None:
    """★★★★ The half that was still wrong after the pan worked."""
    floor = (concession["floor"]["width"], concession["floor"]["height"])
    resize_and_settle(app, floor)
    before = surface_census(app)
    ok(
        f"E/{name}: at the floor the screen agrees with its own paint "
        f"({before['deliverable']} deliverable of {before['painted']} painted, "
        f"{len(before['unreachable'])} unreachable)",
        not before["unreachable"] and before["deliverable"] > 10,
    )
    # Pan by half the range — far enough that a press resolved in the wrong
    # frame lands on something else entirely.
    reach = app.request("scene/scroll_reach")
    assert reach is not None and isinstance(reach.result, dict)
    ranges = [
        row["viewport"]["max_x"]
        for row in reach.result["out_of_sight"]
        if row["viewport"]["name"] == PAN
    ]
    assert ranges, f"E/{name}: the pan is not judging anything at the floor"
    offset = max(ranges) // 2
    app.request("scene/scroll", {"path": PAN, "to": {"x": offset, "y": 0}})
    app.tick(0.016)
    after = surface_census(app)
    ok(
        f"E/{name}: and after panning {offset} pixels it still does "
        f"({after['deliverable']} deliverable of {after['painted']} painted, "
        f"{len(after['unreachable'])} unreachable)",
        not after["unreachable"] and after["deliverable"] > 10,
    )
    # ★★ The pan really moved the paint — otherwise the check above is the same
    # check twice.
    moved = abs_rects_of(app.snapshot(source="paint"))
    app.request("scene/scroll", {"path": PAN, "to": {"x": 0, "y": 0}})
    app.tick(0.016)
    home = abs_rects_of(app.snapshot(source="paint"))
    shifted = [t for t in home if t in moved and home[t][0] != moved[t][0]]
    ok(
        f"E/{name}: and the pan moved what is drawn ({len(shifted)} tagged "
        f"rectangles at a different x)",
        len(shifted) > 5,
    )
    # ★★★★★ R1714.1 — pan it, then GROW THE WINDOW BACK past the layout.
    #
    # The close audit of this round found this and the wire is where it shows: a
    # window big enough for its whole layout is not panned, so nothing paints at
    # an offset — and the offset the reader left behind was still being added to
    # every press. Measured before the repair: 61 painted rectangles, ONE
    # addressable, 61 unreachable. It is checked here rather than only in the
    # unit test because the unit test cannot press anything.
    app.request("scene/scroll", {"path": PAN, "to": {"x": offset, "y": 0}})
    app.tick(0.016)
    resize_and_settle(app, (concession["comfortable"]["width"], floor[1]))
    grown = surface_census(app)
    ok(
        f"E/{name}: and a window grown back past its layout carries no pan with "
        f"it ({grown['deliverable']} deliverable of {grown['painted']} painted, "
        f"{len(grown['unreachable'])} unreachable)",
        not grown["unreachable"] and grown["deliverable"] > 10,
    )


def surface_census(app: RpcSubprocess) -> dict:
    resp = app.request("scene/pointer_target")
    assert resp is not None and isinstance(resp.result, dict)
    surface = resp.result["surfaces"][0]
    return {
        "painted": surface["painted"],
        "deliverable": surface["deliverable"],
        "unreachable": [r["tag"] for r in surface["rows"] if r["verdict"] == "unreachable"],
    }


# ── F: a screen that did not ask for one does not get one ───────────────────


def f_a_screen_that_does_not_declare_a_pan_has_none(
    app: RpcSubprocess, name: str, concession: dict
) -> None:
    floor = (concession["floor"]["width"], concession["floor"]["height"])
    for size in (floor, (floor[0] // 2, floor[1]), (floor[0] - 1, floor[1] - 1)):
        reach = reach_at(app, size)
        named = {row["viewport"]["name"] for row in reach["out_of_sight"]}
        assert PAN not in named, (
            f"F/{name}: a clipping screen has a pan at {size}: {sorted(named)}"
        )
    ok(
        f"F/{name}: no pan is built at any of three sizes, because the policy "
        f"did not ask for one",
        True,
    )
    # ★★ And the round did not move these screens: at their own floor nothing is
    # out of reach, which is the property R1712 and R1713 left them at. The
    # chain answer touches every screen's wire, so this is where a regression in
    # it shows up on a screen that never asked for a pan.
    at_floor = reach_at(app, floor)
    assert_eq(
        at_floor["lost"],
        0,
        f"F/{name}: nothing is out of reach at this screen's own floor",
    )
    scrollable = rows(at_floor, "scrollable")
    ok(
        f"F/{name}: and every mark one gesture away still carries a recipe "
        f"({len(scrollable)} of them, none empty)",
        all(row["moves"] for row in scrollable),
    )
    ok(
        f"F/{name}: naming only viewports this screen has "
        f"({sorted({m['viewport'] for row in scrollable for m in row['moves']})})",
        all(
            m["viewport"] == row["viewport"]["name"]
            for row in scrollable
            for m in row["moves"]
        ),
    )


# ── G: the pixels ───────────────────────────────────────────────────────────


def g_the_pan_is_visible_in_a_photograph(
    app: RpcSubprocess, name: str, concession: dict, tmp: Path
) -> None:
    floor = (concession["floor"]["width"], concession["floor"]["height"])
    resize_and_settle(app, floor)
    home = tmp / "pan-home.png"
    app.request("scene/screenshot", {"path": "", "out_path": str(home)})
    img = read_png_rgba8(home)
    assert_eq(
        (img.width, img.height), floor, f"G/{name}: the photograph is of the floor"
    )
    ok(f"G/{name}: and there is a screen in it ({inked(img)} inked)", inked(img) > 150)
    reach = app.request("scene/scroll_reach")
    assert reach is not None and isinstance(reach.result, dict)
    span = max(
        row["viewport"]["max_x"]
        for row in reach.result["out_of_sight"]
        if row["viewport"]["name"] == PAN
    )
    app.request("scene/scroll", {"path": PAN, "to": {"x": span, "y": 0}})
    app.tick(0.016)
    end = tmp / "pan-end.png"
    app.request("scene/screenshot", {"path": "", "out_path": str(end)})
    far = read_png_rgba8(end)
    ok(
        f"G/{name}: panned to the far end ({span}) there is still a screen "
        f"({inked(far)} inked)",
        inked(far) > 150,
    )
    # ★★ The two photographs are different — a pan that painted the same pixels
    # would satisfy every count above.
    differing = sum(
        1
        for y in range(4, min(img.height, far.height), 11)
        for x in range(2, min(img.width, far.width), 5)
        if png_pixel(img, x, y) != png_pixel(far, x, y)
    )
    ok(
        f"G/{name}: and it is a DIFFERENT screen ({differing} sampled pixels "
        f"changed between the two)",
        differing > 40,
    )
    app.request("scene/scroll", {"path": PAN, "to": {"x": 0, "y": 0}})
    app.tick(0.016)
    print(f"[demo] G/{name}: {floor[0]}x{floor[1]} -> {home.name}, {end.name}")


def inked(img: Png) -> int:
    count = 0
    for row in range(4, img.height, 11):
        for col in range(2, img.width, 5):
            r, g, b, _ = png_pixel(img, col, row)
            if abs(r - g) > 6 or abs(g - b) > 6 or r > 60:
                count += 1
    return count


# ── the round ───────────────────────────────────────────────────────────────


def drive(name: str, example: str, tmp: Path) -> None:
    banner(f"{name} ({example})")
    with RpcSubprocess(example) as app:
        opened = design_size(app)
        concession = a_the_recourse_is_published(app, name, example)
        if example == PANNING:
            b_nothing_is_out_of_reach_anywhere_in_the_band(app, name, concession)
            assert_eq(
                design_size(app),
                opened,
                f"F/{name}: and none of those asks moved the window (§2 #3)",
            )
            c_the_recipe_names_the_pan_and_works(app, name)
            d_a_1600_wide_display_holds_the_screen(app, name, concession)
            e_a_press_lands_after_the_pan(app, name, concession)
            g_the_pan_is_visible_in_a_photograph(app, name, concession, tmp)
        else:
            f_a_screen_that_does_not_declare_a_pan_has_none(app, name, concession)
            assert_eq(
                design_size(app),
                opened,
                f"F/{name}: and none of those asks moved the window (§2 #3)",
            )
        floor = (concession["floor"]["width"], concession["floor"]["height"])
        resize_and_settle(app, floor)
        declared = declared_and_painted(app, floor)
        ok(
            f"G/{name}: the specification is still on screen at the floor "
            f"({len(declared)} regions)",
            len(declared) >= 8,
        )


def main() -> None:
    with tempfile.TemporaryDirectory() as d:
        for name, example in SCREENS:
            drive(name, example, Path(d))
    print(f"\n{len(CHECKS)} assertions across {len(SCREENS)} screens")
    assert len(CHECKS) >= 30, f"only {len(CHECKS)} assertions"


if __name__ == "__main__":
    sys.exit(run_demo(Path(__file__).stem, main))
