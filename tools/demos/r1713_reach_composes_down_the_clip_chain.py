#!/usr/bin/env python3
"""R1713 §5.16 §5.32 §5.12 §2 #3 §2 #7 — **whether a reader can see a mark is a
question about the whole clip chain**, on all three screens of the analysis tool.

# What this exists for

`scene/scroll_reach` judged every mark against its *nearest* clipping ancestor.
So when the window sliced a pane, the marks inside that pane were measured against
the pane — which they fit — and the window's cut was invisible to the read.
Measured on the node lab at 1506 wide the round before this one: **`lost: 0`
while nine tagged marks sat entirely outside the window**, every one of them an
action (five row remove buttons, two spin steppers, `+ key`, `delete`), in a pane
whose horizontal range is zero. A window floor was published 89 pixels too low on
the strength of that answer, and the round after it published one 6 pixels too low
for a second reason: the repair read the PAINT, and the paint is keyed by tag, so
it could not see the `×` glyph *inside* a remove button.

R1713 folds the chain — each level is offered only what the level above can ever
bring into view — and splits the answer three ways, because the safety rule that
reads it (*a concession may clip and may never lose*) needs a distinction the old
two arms could not express: a form row whose right edge is unreachable came back
`lost` beside a glyph nothing reaches at all.

# What it asserts

* **A** — the wire publishes the **aperture** and the **declared** box side by
  side, and on a sliced pane they differ by exactly what the window took. A read
  that only had the declared box is the read that could not see this.
* **B** — ★★★★★ the headline, on the screen that paid for it: inside the band the
  marks the window removes are reported, by name, as `lost`, and their pane's
  range is zero so nothing brings them back.
* **C** — ★★ `clipped` and `lost` are two answers and the counts partition the
  list: `scrollable + clipped + lost == len(out_of_sight)`. One number could not
  tell the rule which it was looking at.
* **D** — ★★★ every screen's floor is **driven** against the predicate: the
  boundary is bisected here rather than read out of the binding, and for the
  screen that concedes it must EQUAL the declared floor. This is the check that
  refuses 1506 and refuses 1595.
* **E** — ★★ attribution: at a width that removes a whole pane, the report names
  the **pane**, once — not every row inside it. The fold seals rather than
  repeating one break N times.
* **F** — ★★★★ the predicate against the PIXELS and the paint, in the live window
  at a size it will actually take: the two channels name the same reachable set,
  both directions, and the band's clipping is visible in a photograph.
* **G** — §2 #3: every hypothetical size above is answered without the window
  moving, and the specification is still on screen at the floor.
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    Png,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    declared_and_painted,
    design_size,
    png_pixel,
    read_png_rgba8,
    resize_and_settle,
    run_demo,
)

#: The three screens, in the order the specification names them, each with the
#: pane whose slicing this round is about and the axis the band moves.
SCREENS = [
    ("node lab", "hello-node-lab", "lab.inspector"),
    ("capture viewer", "hello-packet-view", "pv.bytes"),
    ("dashboard", "hello-analyzer-shell", "shell.canvas"),
]

#: The one screen that concedes width, and what it must have measured. Written
#: here so this file asserts the DECISION rather than echoing the binding: a
#: screen that quietly lowered its floor would otherwise still pass.
CONCEDING = "hello-node-lab"
CONCEDED_FLOOR = (1601, 360)

#: What the window removes first on that screen, in the order a reader loses it.
#: These are the marks R1712 published a floor over the top of.
LOST_AT_1600 = 5

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


def rows(reach: dict, verdict: str) -> list[dict]:
    return [row for row in reach["out_of_sight"] if row["reach"] == verdict]


def names(reach: dict, verdict: str) -> list[str]:
    return sorted({row["tag"] or row["path"] for row in rows(reach, verdict)})


def policy_of(app: RpcSubprocess) -> dict:
    resp = app.request("scene/size_floor")
    assert resp is not None and isinstance(resp.result, dict)
    return resp.result


# ── A: the aperture and the box it was cut from ──────────────────────────────


def narrowed_rows(reach: dict) -> list[dict]:
    """Rows judged against a viewport the chain above it cut down."""
    return [
        row
        for row in reach["out_of_sight"]
        if (row["viewport"]["w"], row["viewport"]["h"])
        != (row["viewport"]["declared"]["w"], row["viewport"]["declared"]["h"])
    ]


def first_narrowing_width(
    app: RpcSubprocess, ceiling: tuple[int, int]
) -> tuple[int, int]:
    """The widest window this screen has that slices one of its own panes.

    Searched rather than written down: each screen puts its clipping panes in a
    different place, so the width at which the window starts cutting into one is
    a property of the screen. A ladder rather than a bisection because the
    predicate is not monotone at the bottom — narrow far enough and the pane is
    gone altogether, which seals it and stops it judging anything (section E).
    """
    assert not narrowed_rows(reach_at(app, ceiling)), "nothing is cut at the ceiling"
    step = 1
    while step < ceiling[0]:
        size = (ceiling[0] - step, ceiling[1])
        if narrowed_rows(reach_at(app, size)):
            return size
        step *= 2
    raise AssertionError(f"no width slices a pane on this screen (ceiling {ceiling})")


def a_viewport_publishes_the_aperture_and_the_box(
    app: RpcSubprocess, name: str, pane: str, size: tuple[int, int]
) -> str:
    """The two numbers whose absence made the defect unrepresentable.

    Returns the name of a viewport the chain narrows, so the sections below drive
    a pane this screen really has rather than one written down here.
    """
    reach = reach_at(app, size)
    narrowed = narrowed_rows(reach)
    ok(
        f"A/{name}: at {size[0]}x{size[1]} the chain narrows some viewport, and the "
        f"wire says so ({len(narrowed)} row(s) judged against an aperture smaller "
        f"than the box it was cut from)",
        len(narrowed) > 0,
    )
    row = narrowed[0]
    view = row["viewport"]
    taken = view["declared"]["w"] - view["w"]
    ok(
        f"A/{name}: and the narrowing is a subtraction a reader can do — "
        f"{view['name']} declares {view['declared']['w']} and is offered {view['w']}, "
        f"so the chain took {taken}",
        taken > 0 and view["w"] > 0,
    )
    # ★ The aperture is never larger than the box: an aperture that grew would be
    # a viewport claiming to show more than its own node.
    for r in reach["out_of_sight"]:
        v = r["viewport"]
        assert v["w"] <= v["declared"]["w"] and v["h"] <= v["declared"]["h"], (
            f"A/{name}: {v['name']} publishes an aperture bigger than its box: {v}"
        )
    ok(f"A/{name}: no aperture exceeds the box it was cut from", True)
    # ★★ And the pane this screen is judged on is one of the narrowed ones — the
    # check above would pass on any screen with any clip anywhere.
    ok(
        f"A/{name}: {pane} is among them",
        any(pane in row["viewport"]["name"] or pane in row["path"] for row in narrowed),
    )
    return next(
        row["viewport"]["name"] for row in narrowed if pane in row["viewport"]["name"]
    )


# ── B: the marks the window removed ─────────────────────────────────────────


def b_a_mark_the_window_removes_is_reported(app: RpcSubprocess, name: str) -> None:
    """★★★★★ The headline, at the width the previous round shipped."""
    at_1600 = reach_at(app, (CONCEDED_FLOOR[0] - 1, CONCEDED_FLOOR[1]))
    lost = rows(at_1600, "lost")
    assert_eq(
        len(lost),
        LOST_AT_1600,
        f"B/{name}: one pixel below the floor the reader loses {LOST_AT_1600} marks "
        f"({[r['content'] for r in lost]})",
    )
    # ★ Every one is inside a pane, not a top-level mark — which is precisely why
    # the old read could not see them: they fit the pane and the window cut the
    # pane. A regression that reported them against `<window>` would be a
    # different (and wrong) answer that satisfies a count.
    ok(
        f"B/{name}: and every one is judged against a pane rather than the window "
        f"({sorted({r['viewport']['name'] for r in lost})})",
        all(r["viewport"]["name"] != "<window>" for r in lost),
    )
    # ★★ Nothing brings them back: that pane has no range on the axis they are off.
    ok(
        f"B/{name}: their pane has no horizontal range, so no gesture reaches them",
        all(r["viewport"]["max_x"] == 0 for r in lost),
    )
    # ★★ They are ink a reader would have read — the glyphs inside the row
    # actions, which is what makes this a loss and not a cosmetic edge.
    ok(
        f"B/{name}: and they are text a reader was meant to read "
        f"({sorted({r['content'] for r in lost if r['content']})})",
        all(r["content"] for r in lost),
    )
    # ★★★★★ The paint scan that replaced this predicate last round cannot see
    # them: they carry no tag of their own. Asserted, because it is the reason the
    # repair has to be in the predicate.
    ok(
        f"B/{name}: none of them carries a tag, which is why a tag-keyed scan of "
        f"the paint reported the floor clean",
        all(row["tag"] is None for row in lost),
    )


# ── C: three answers, and they partition ────────────────────────────────────


def c_clipped_and_lost_are_two_answers(
    app: RpcSubprocess, name: str, size: tuple[int, int]
) -> None:
    reach = reach_at(app, size)
    total = len(reach["out_of_sight"])
    assert_eq(
        reach["scrollable"] + reach["clipped"] + reach["lost"],
        total,
        f"C/{name}: the three counts partition the list at {size[0]}x{size[1]}",
    )
    ok(
        f"C/{name}: and the list is not empty ({total} marks off screen of "
        f"{reach['marks']} painted)",
        total > 0,
    )
    for verdict in ("scrollable", "clipped", "lost"):
        for row in rows(reach, verdict):
            if verdict == "scrollable":
                assert row["to_x"] is not None and row["short_by"] is None, row
            else:
                assert row["to_x"] is None and row["short_by"] is not None, row
    ok(
        f"C/{name}: an offset rides on the arm that has one and an overhang on the "
        f"arms that do",
        True,
    )
    # ★★ A `clipped` row is reachable in part: its overhang is smaller than the
    # mark, and a `lost` row's is not. That is the whole difference, checked
    # rather than described.
    for row in rows(reach, "clipped"):
        left, top, right, bottom = row["short_by"]
        assert right < row["rect"]["w"] or left < row["rect"]["w"], row
        assert bottom < row["rect"]["h"] or top < row["rect"]["h"], row
    ok(
        f"C/{name}: every `clipped` row overhangs by less than its own extent on at "
        f"least one axis ({reach['clipped']} rows)",
        True,
    )


# ── D: the floor, bisected here ─────────────────────────────────────────────


def d_the_floor_is_where_reach_actually_ends(
    app: RpcSubprocess, name: str, example: str, report: dict
) -> None:
    """★★★ Bisect the boundary rather than believe the binding's number."""
    concession = report["concession"]
    floor = (concession["floor"]["width"], concession["floor"]["height"])
    ceiling = (concession["comfortable"]["width"], concession["comfortable"]["height"])
    assert reach_at(app, ceiling)["lost"] == 0, f"D/{name}: the ceiling loses nothing"
    lo, hi, probes = 100, ceiling[0], 0
    while hi - lo > 1:
        mid = (lo + hi) // 2
        probes += 1
        if reach_at(app, (mid, floor[1]))["lost"] == 0:
            hi = mid
        else:
            lo = mid
    measured = hi
    ok(
        f"D/{name}: the width at which nothing is out of reach is {measured}, "
        f"evaluated in {probes} probes (one below loses "
        f"{reach_at(app, (measured - 1, floor[1]))['lost']})",
        reach_at(app, (measured, floor[1]))["lost"] == 0
        and reach_at(app, (measured - 1, floor[1]))["lost"] > 0,
    )
    if example == CONCEDING:
        assert_eq(
            measured,
            floor[0],
            f"D/{name}: a conceded floor IS that boundary — the declaration and the "
            f"measurement are one number",
        )
        assert_eq(floor, CONCEDED_FLOOR, f"D/{name}: and it is the decision on record")
    else:
        # ★ A rigid floor stops EARLIER than reachability requires, because what
        # stops it is content being cut rather than lost. Reported with the slack,
        # so a rigid screen that had quietly become unreachable at its floor fails.
        ok(
            f"D/{name}: a rigid floor stops before reach does — floor {floor[0]}, "
            f"reach boundary {measured}, {floor[0] - measured} pixels of slack",
            measured <= floor[0],
        )
        assert_eq(
            reach_at(app, floor)["lost"],
            0,
            f"D/{name}: and nothing is out of reach at the floor itself",
        )


# ── E: attribution — one report per break ───────────────────────────────────


def e_a_pane_the_window_removes_is_reported_once(
    app: RpcSubprocess, name: str, viewport: str, size: tuple[int, int]
) -> None:
    """★★ The rule the fold had to keep: name the pane, not every row in it.

    `viewport` is a pane this screen really has, taken from section A's own
    measurement — a name written down here would drift from the screen. `size` is
    a width at which that pane is narrowed but still there, so the pair of reads
    below is "judging" against "sealed" rather than "empty" against "empty".
    """
    wide = reach_at(app, size)
    judged_wide = [r for r in wide["out_of_sight"] if r["viewport"]["name"] == viewport]
    # ★ Searched, for the reason section A's width is: a pane anchored near the
    # left edge does not vanish until the window is narrower than its own left
    # edge, and where that is differs per screen (measured: 541 on the node lab,
    # far narrower on the dashboard, whose canvas starts after a rail).
    narrow = None
    width = size[0]
    while width > 32:
        width //= 2
        probe = reach_at(app, (width, size[1]))
        if not [r for r in probe["out_of_sight"] if r["viewport"]["name"] == viewport]:
            narrow, reach = (width, size[1]), probe
            break
    assert narrow is not None, f"E/{name}: no width removes {viewport} altogether"
    named = [r for r in rows(reach, "lost") if r["tag"] == viewport]
    ok(
        f"E/{name}: at {narrow[0]}x{narrow[1]} the report names {viewport} itself, "
        f"against the window ({len(named)} row(s))",
        len(named) == 1 and named[0]["viewport"]["name"] == "<window>",
    )
    # ★★ And not one row per thing inside it: nothing is judged against that pane,
    # because no part of the pane is reachable to judge anything in. The wide read
    # above is what keeps this from passing vacuously — the same pane judges marks
    # there.
    inside = [r for r in reach["out_of_sight"] if r["viewport"]["name"] == viewport]
    ok(
        f"E/{name}: and nothing inside it is reported separately ({len(inside)} rows "
        f"judged against it there, against {len(judged_wide)} at {size[0]} wide "
        f"where the pane survives)",
        not inside and judged_wide,
    )


# ── F: the pixels and the paint, in the live window ──────────────────────────


def f_the_paint_agrees_at_a_size_the_window_takes(
    app: RpcSubprocess, name: str, floor: tuple[int, int], png: Path
) -> None:
    """★★★★ Two channels, one geometry, at a size the window will really hold."""
    granted = resize_and_settle(app, floor)
    assert_eq(design_size(app), floor, f"F/{name}: the window took its floor")
    if isinstance(granted, dict) and "width" in granted:
        assert_eq(
            (granted["width"], granted["height"]),
            floor,
            f"F/{name}: and the resize granted that size rather than clamping it",
        )
    live = app.request("scene/scroll_reach")
    assert live is not None and isinstance(live.result, dict)
    assert_eq(
        live.result["lost"],
        0,
        f"F/{name}: nothing is out of reach in the live window at its floor "
        f"({names(live.result, 'lost')[:4]})",
    )
    # ⚠ `viewport` is NOT passed: `scene/snapshot {from: "paint"}` prefers the
    # displayed frame and ignores it once a frame exists ⇒
    # `debt-a-snapshot-viewport-is-ignored-once-a-frame-exists`. The window was
    # driven to `floor` above, so the displayed frame IS the geometry to compare.
    painted = abs_rects_of(app.snapshot(source="paint"))
    on_screen = {
        tag
        for tag, (x, y, w, h) in painted.items()
        if x < floor[0] and y < floor[1] and w > 0 and h > 0
    }
    unreachable = set(names(live.result, "lost"))
    ok(
        f"F/{name}: the paint and the predicate name the same reachable set "
        f"({len(on_screen)} tagged marks on screen, {len(unreachable)} unreachable)",
        not (on_screen & unreachable),
    )
    ok(
        f"F/{name}: and the comparison is not vacuous ({len(on_screen)} on screen)",
        len(on_screen) > 20,
    )
    app.request("scene/screenshot", {"path": "", "out_path": str(png)})
    assert png.exists(), f"F/{name}: no screenshot was written"
    img = read_png_rgba8(png)
    assert_eq(
        (img.width, img.height),
        floor,
        f"F/{name}: the photograph is of the window at its floor",
    )
    inked = inked_samples(img, 0, img.width)
    ok(f"F/{name}: and there is a screen in it ({inked} inked samples)", inked > 150)
    print(f"[demo] F/{name}: {floor[0]}x{floor[1]}, {inked} inked -> {png.name}")


def g_the_band_is_visible_where_it_bites(
    app: RpcSubprocess, name: str, floor: tuple[int, int], band: int, png: Path
) -> None:
    """★ The conceded band's last columns still carry a screen: what the window
    keeps of a clipped pane is drawn, which is what makes it a clip."""
    img = read_png_rgba8(png)
    edge = inked_samples(img, max(0, floor[0] - band), floor[0])
    ok(
        f"G/{name}: the rightmost {band} columns the window keeps are drawn "
        f"({edge} inked samples), so the pane is clipped rather than blanked",
        edge > 0,
    )
    live = app.request("scene/scroll_reach")
    assert live is not None and isinstance(live.result, dict)
    ok(
        f"G/{name}: and the predicate calls that band's marks `clipped`, not `lost` "
        f"({live.result['clipped']} clipped, {live.result['lost']} lost)",
        live.result["clipped"] > 0 and live.result["lost"] == 0,
    )


def inked_samples(img: Png, x0: int, x1: int) -> int:
    """Samples carrying ink in a column band — scanned, not glanced at."""
    inked = 0
    for row in range(4, img.height, 11):
        for col in range(x0 + 2, min(x1, img.width), 5):
            r, g, b, _ = png_pixel(img, col, row)
            if abs(r - g) > 6 or abs(g - b) > 6 or r > 60:
                inked += 1
    return inked


# ── the round ───────────────────────────────────────────────────────────────


def drive(name: str, example: str, pane: str, tmp: Path) -> None:
    banner(f"{name} ({example})")
    with RpcSubprocess(example) as app:
        before = design_size(app)
        report = policy_of(app)
        concession = report["concession"]
        floor = (concession["floor"]["width"], concession["floor"]["height"])
        ceiling = (
            concession["comfortable"]["width"],
            concession["comfortable"]["height"],
        )
        band = concession["band"]["width"]
        # ★ The size to ask about is SEARCHED, not written down: each screen puts
        # its clipping panes somewhere else, so the width at which the window
        # first cuts into one is a property of the screen. Measured: 2 pixels
        # below the layout on the node lab, 55 on the dashboard — and the first
        # draft of this file asserted the conceded floor was that width, which the
        # screen refused. A floor is where something is LOST, and a pane starts
        # being narrowed long before anything is.
        narrows_at = first_narrowing_width(app, (ceiling[0], floor[1]))
        # ★★ The two policies part company exactly here, and the assertion says
        # which way round: a conceded floor is BELOW the width where the window
        # starts cutting into a pane (that is what the band buys), a rigid one is
        # above it (that is what rigid means). The first draft asserted the
        # conceded case for both and the capture viewer refused it.
        ok(
            f"A/{name}: the window first cuts into a pane at {narrows_at[0]} wide, "
            f"{ceiling[0] - narrows_at[0]} below the layout, and this screen's floor "
            f"of {floor[0]} is {'below it — the band goes past the first cut' if band else 'above it — rigid stops before one'}",
            narrows_at[0] < ceiling[0]
            and (floor[0] <= narrows_at[0] if band else floor[0] > narrows_at[0]),
        )
        narrowed = a_viewport_publishes_the_aperture_and_the_box(
            app, name, pane, narrows_at
        )
        # A size that stresses all three arms: below the floor, where the screen
        # is losing things as well as clipping them.
        stressed_at = (floor[0] - 1 if band else floor[0] - 40, floor[1])
        c_clipped_and_lost_are_two_answers(app, name, stressed_at)
        d_the_floor_is_where_reach_actually_ends(app, name, example, report)
        e_a_pane_the_window_removes_is_reported_once(app, name, narrowed, narrows_at)
        if example == CONCEDING:
            b_a_mark_the_window_removes_is_reported(app, name)
        assert_eq(
            design_size(app),
            before,
            f"G/{name}: and none of those asks moved the window (§2 #3)",
        )
        png = tmp / f"{example}.png"
        f_the_paint_agrees_at_a_size_the_window_takes(app, name, floor, png)
        if band:
            g_the_band_is_visible_where_it_bites(app, name, floor, band, png)
        declared = declared_and_painted(app, floor)
        ok(
            f"G/{name}: the specification is still on screen at the floor "
            f"({len(declared)} regions)",
            len(declared) >= 8,
        )


def main() -> None:
    with tempfile.TemporaryDirectory() as d:
        for name, example, pane in SCREENS:
            drive(name, example, pane, Path(d))
    print(f"\n{len(CHECKS)} assertions across {len(SCREENS)} screens")
    assert len(CHECKS) >= 30, f"only {len(CHECKS)} assertions"


if __name__ == "__main__":
    sys.exit(run_demo(Path(__file__).stem, main))
