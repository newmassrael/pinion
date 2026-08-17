#!/usr/bin/env python3
"""R1712 §5.16 §5.32 §5.12 §2 #3 §2 #7 — **a window says what it gives up to
get smaller**, on all three screens of the analysis tool.

# What this exists for

A screen has two minimums: the size below which its layout stops reflowing, and
the size below which its window refuses to shrink. Measured before this round,
all three screens here passed **one constant to both** — not tidiness, but the
absence of a way to say anything else. R1689 wrote the cost down at the time:
raising the node lab's toolbar floor to 1625 meant *"a 1600-wide display no
longer holds this screen. That is a real loss."* It stayed a loss for 23 rounds
because lowering the number would have moved the layout too.

`ShrinkPolicy` splits them and makes the second number a **declaration**: below
its floor the node lab clips its app bar's right end and its inspector, and says
so. This file is what keeps that declaration honest — every section below is
computed from what the screen answers, so it drives three screens whose
policies genuinely differ (one concedes, two are rigid).

# What it asserts

* **A** — every screen declares a policy, and the wire tells a *decision* from a
  *default*: `rigid` is somebody's answer, an absent policy is nobody's.
* **B** — ★ the two floors are two numbers, and the audit at the floor is
  `honoured`: what is clipped there is exactly what the binding named, nothing
  more (`unnamed` empty — the defect direction) and nothing less (`stale`
  empty).
* **C** — ★★ the floor is a **boundary in the reach predicate**, driven in both
  directions: at the floor nothing is out of the reader's reach, and one pixel
  below something is, by name. A floor nobody drives is a number somebody typed.
* **D** — ★★★ the payoff, in the window: the node lab really opens and resizes at
  its conceded floor. The granted size, the painted rectangle and the
  specification all agree there.
* **E** — a concession clips and never loses: at the floor `scene/scroll_reach`
  reports zero `lost`, and every region the policy gives up is still *painted*
  and still *addressable by a pointer*. Content behind a concession is content
  the reader can still get at.
* **F** — a rigid screen's floor cuts nothing at all, which is what makes
  `rigid` a stronger statement than a bare minimum size.
* **G** — §2 #3: the whole audit is answered without the window moving, and a
  declaration that disagrees with what the window system was told would say so
  (`declaration_split`).
* **H** — ★ pixels. At its conceded floor the node lab is photographed and
  scanned, and the scan looks *where the concession is*: the right end of the app
  bar is gone and the left of the screen is still a screen.
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
    assert_targets_survive_resize,
    declared_and_painted,
    design_size,
    png_pixel,
    read_png_rgba8,
    resize_and_settle,
    run_demo,
)

#: The three screens of the tool, in the order the specification names them.
SCREENS = [
    ("node lab", "hello-node-lab"),
    ("capture viewer", "hello-packet-view"),
    ("dashboard", "hello-analyzer-shell"),
]

#: The one screen that concedes, and what it declares the band costs. Written
#: here so the file asserts the *decision* rather than echoing whatever the
#: binding currently says — an audit that reads its expectation out of the
#: thing under test passes for a screen that changed its mind quietly.
CONCEDING = "hello-node-lab"
#: ★ R1713 re-measured this: 1506 (R1712) and 1595 (R1712.1) were both taken with
#: a predicate that could not see a mark inside a pane the window slices. With the
#: clip chain folded and `clipped` split from `lost`, the boundary is 1601 — the
#: width at which nothing is `lost`, with five row `×` glyphs lost at 1600.
CONCEDED_FLOOR = (1601, 360)
GIVES_UP = ["lab.appbar", "lab.inspector"]

CHECKS: list[str] = []


def ok(what: str, condition: bool) -> None:
    assert condition, f"FAILED: {what}"
    CHECKS.append(what)


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def floor_of(app: RpcSubprocess) -> dict:
    resp = app.request("scene/size_floor")
    assert resp is not None and isinstance(resp.result, dict), "scene/size_floor answers"
    return resp.result


def reach_at(app: RpcSubprocess, size: tuple[int, int]) -> dict:
    resp = app.request(
        "scene/scroll_reach", {"at": {"width": size[0], "height": size[1]}}
    )
    assert resp is not None and isinstance(resp.result, dict), "scroll_reach answers"
    return resp.result


def lost_names(reach: dict) -> list[str]:
    return sorted(
        {
            row["tag"] or row["path"]
            for row in reach["out_of_sight"]
            if row["reach"] == "lost"
        }
    )


def cut_names(rows: list[dict]) -> list[str]:
    return sorted({row["tag"] or row["path"] for row in rows})


def beyond_the_window(app: RpcSubprocess, size: tuple[int, int]) -> list[str]:
    """Tagged marks the LIVE paint puts entirely outside a window of `size`.

    ⚠ R1713 — `viewport` is deliberately not passed. `scene/snapshot
    {from: "paint"}` prefers the displayed frame and **ignores** `viewport`
    whenever a frame has been painted (measured: five different asks, two
    screens, the same live root every time) ⇒
    `debt-a-snapshot-viewport-is-ignored-once-a-frame-exists`. R1712.1's boundary
    check passed that parameter and believed it was measuring a hypothetical
    size; it was measuring the live one, which happens to be the same geometry on
    this screen (its layout is clamped at its design width) and is NOT on the
    capture viewer. So the caller's contract here is: drive the window to `size`
    first, then ask.

    The clip stack folded into `abs_rects_of` is what the tree DECLARES, and the
    window is not a declared clip, so the intersection with `size` is done here.
    """
    rects = abs_rects_of(app.snapshot(source="paint"))
    return sorted(
        tag
        for tag, (x, y, w, h) in rects.items()
        if x >= size[0] or y >= size[1] or x + w <= 0 or y + h <= 0
    )


# ── A: a decision, and how it differs from a default ────────────────────────


def the_policy_is_declared_and_says_which_kind_it_is(
    app: RpcSubprocess, name: str, example: str
) -> dict:
    report = floor_of(app)
    concession = report.get("concession")
    ok(f"A/{name}: the binding declares a shrink policy at all", concession is not None)
    band = (concession["band"]["width"], concession["band"]["height"])
    concedes = band != (0, 0)
    assert_eq(
        concedes,
        example == CONCEDING,
        f"A/{name}: the wire says whether this screen concedes anything",
    )
    # ★ The distinction the type exists for: `rigid` is a decision that the
    # window stops where the layout does, and it is NOT the same wire answer as
    # a binding that declares nothing. Both floors present, band zero, and an
    # empty `gives_up` is what a decision to concede nothing looks like.
    if not concedes:
        assert_eq(
            concession["comfortable"],
            concession["floor"],
            f"A/{name}: a rigid policy puts both floors at one size",
        )
        assert_eq(
            concession["gives_up"],
            [],
            f"A/{name}: and gives nothing up, because there is no band to do it in",
        )
    assert_eq(
        report["verdict"],
        "conceded" if concedes else "exact",
        f"A/{name}: the top-level verdict reads the two apart",
    )
    print(
        f"[demo] A/{name}: comfortable "
        f"{concession['comfortable']['width']}x{concession['comfortable']['height']}, "
        f"floor {concession['floor']['width']}x{concession['floor']['height']}, "
        f"band {band[0]}x{band[1]}, verdict {concession['verdict']}"
    )
    return report


# ── B: the audit at the floor ───────────────────────────────────────────────


def what_is_clipped_is_what_was_declared(name: str, example: str, report: dict) -> None:
    concession = report["concession"]
    assert_eq(
        concession["verdict"],
        "honoured",
        f"B/{name}: the declaration matches the screen at its own floor",
    )
    assert_eq(
        concession["unreachable"],
        [],
        f"B/{name}: nothing is out of reach at the floor — a concession clips, "
        f"it does not lose",
    )
    # ★★★★★ The defect direction: a screen giving up something it never
    # admitted to. Named rather than counted, because a count cannot be looked
    # up and a name can.
    assert_eq(
        concession["unnamed"],
        [],
        f"B/{name}: nothing is clipped that the binding did not name",
    )
    assert_eq(
        concession["stale"],
        [],
        f"B/{name}: and nothing is named that is no longer clipped",
    )
    if example == CONCEDING:
        assert_eq(
            (concession["floor"]["width"], concession["floor"]["height"]),
            CONCEDED_FLOOR,
            f"B/{name}: the floor is the one this round decided on",
        )
        assert_eq(
            sorted(concession["gives_up"]),
            sorted(GIVES_UP),
            f"B/{name}: and it gives up the two regions it decided to give up",
        )
        # A declaration made of REGION names covers the runs inside them, so the
        # count it bought is published rather than left for a reader to guess.
        assert_eq(
            concession["covered"],
            len(concession["cut_at_floor"]),
            f"B/{name}: every clipped mark is accounted for by a declared region",
        )
        ok(
            f"B/{name}: and the two names cover more than themselves "
            f"({concession['covered']} marks from {len(GIVES_UP)} names: "
            f"{cut_names(concession['cut_at_floor'])})",
            concession["covered"] > len(GIVES_UP),
        )
        for row in concession["cut_at_floor"]:
            ok(
                f"B/{name}: {row['tag'] or row['path']} says how far past the "
                f"window it reaches ({row['short_by']})",
                any(edge > 0 for edge in row["short_by"]),
            )


# ── C: the floor is a boundary, driven both ways ────────────────────────────


def the_floor_is_where_reach_actually_ends(
    app: RpcSubprocess, name: str, report: dict
) -> None:
    concession = report["concession"]
    floor = (concession["floor"]["width"], concession["floor"]["height"])
    band = (concession["band"]["width"], concession["band"]["height"])
    before = design_size(app)
    at_floor = reach_at(app, floor)
    assert_eq(
        at_floor["lost"],
        0,
        f"C/{name}: at the declared floor the reader can still reach everything "
        f"({lost_names(at_floor)})",
    )
    ok(
        f"C/{name}: and the read examined a screen's worth of marks "
        f"({at_floor['marks']})",
        at_floor["marks"] > 100,
    )
    # ★★★★★ The half that makes the number falsifiable, and it is asked in the
    # predicate the policy BINDS THAT AXIS ON — the first draft asked reach for
    # both and the capture viewer failed it correctly. A rigid axis stops where
    # the screen can no longer show everything **whole**, so one pixel below it
    # something is cut; a conceded axis has deliberately gone past that, so what
    # stops it is the reader no longer being able to **reach** something.
    # Without this a floor of 1 would satisfy every assertion above: nothing is
    # ever lost when there is nothing on screen to lose.
    #
    # ★★★★★ R1713 — the conceded half asks `scroll_reach` again, and that is the
    # round's payoff rather than a relapse. R1712.1 moved it to a scan of the
    # PAINT because the predicate was blind: it judged each mark against its
    # nearest scrolling ancestor, so a mark inside a pane the window slices fitted
    # *the pane* and was never reported (`lost: 0` at 1506, with nine actions
    # entirely off the window). The predicate now folds the clip chain, and the
    # paint scan turned out to be the weaker instrument of the two — it is
    # tag-keyed, so it cannot see the `×` glyph inside a remove button, and those
    # glyphs are what goes first here. Section J checks the two against each
    # other at a size the window can actually take.
    for index, axis in enumerate(("width", "height")):
        short = list(floor)
        short[index] -= 1
        if band[index] > 0:
            below = reach_at(app, (short[0], short[1]))
            ok(
                f"C/{name}/{axis}: this axis is conceded, and one pixel below its "
                f"floor {below['lost']} mark(s) are out of the reader's reach "
                f"altogether ({lost_names(below)[:3]})",
                below["lost"] > 0,
            )
            ok(
                f"C/{name}/{axis}: and at the floor those same marks are reachable, "
                f"so the boundary is this pixel and not a smaller one",
                at_floor["lost"] == 0,
            )
        else:
            resp = app.request(
                "scene/size_floor", {"at": {"width": short[0], "height": short[1]}}
            )
            assert resp is not None and isinstance(resp.result, dict)
            refused = resp.result.get("refused")
            ok(
                f"C/{name}/{axis}: this axis is rigid, and one pixel below its "
                f"floor something is cut "
                f"({cut_names(refused['out_of_reach']) if refused else 'nothing'})",
                refused is not None and refused["reason"] == "ceiling_is_short",
            )
    assert_eq(
        design_size(app),
        before,
        f"C/{name}: and none of those asks moved the window (§2 #3)",
    )


# ── D: the payoff, in the real window ───────────────────────────────────────


def the_window_really_goes_there(app: RpcSubprocess, name: str, report: dict) -> tuple[int, int]:
    concession = report["concession"]
    floor = (concession["floor"]["width"], concession["floor"]["height"])
    granted = resize_and_settle(app, floor)
    assert_eq(design_size(app), floor, f"D/{name}: the window took its declared floor")
    if isinstance(granted, dict) and "width" in granted:
        assert_eq(
            (granted["width"], granted["height"]),
            floor,
            f"D/{name}: and the resize GRANTED that size rather than clamping it",
        )
    declared = declared_and_painted(app, floor)
    ok(
        f"D/{name}: the specification is on screen there ({len(declared)} regions)",
        len(declared) >= 8,
    )
    return floor


# ── E: a concession clips, and never loses ──────────────────────────────────


def what_is_given_up_is_still_reachable(
    app: RpcSubprocess, name: str, example: str, size: tuple[int, int]
) -> None:
    live = app.request("scene/scroll_reach")
    assert live is not None and isinstance(live.result, dict)
    assert_eq(
        live.result["lost"],
        0,
        f"E/{name}: nothing is out of reach in the live window at its floor",
    )
    # ★★★★★ R1712.1 — and the same claim read off the GEOMETRY, because the
    # line above cannot establish it. This is the check that would have refused
    # the floor this round first shipped. R1713: the window really is `size` here
    # (D resized it), so the live paint is the right geometry to ask.
    assert_eq(
        beyond_the_window(app, size),
        [],
        f"E/{name}: and no mark is outside the window altogether at the floor — "
        f"a concession clips, it does not put things out of the reader's reach",
    )
    # ★★★★★ R1713 — the two channels, compared mark by mark rather than by
    # number. The predicate folds the clip chain and the paint scan folds the
    # declared clips; they are two derivations of "can the reader see this", and
    # the round that separated them found the wire answering `lost: 0` about nine
    # marks the paint put outside the window. Both directions are asserted, so a
    # wire that reported everything and a wire that reported nothing both fail.
    painted = abs_rects_of(app.snapshot(source="paint"))
    on_screen = {
        tag
        for tag, (x, y, w, h) in painted.items()
        if x < size[0] and y < size[1] and w > 0 and h > 0
    }
    unreachable_per_wire = set(lost_names(live.result))
    ok(
        f"E/{name}: the paint and the predicate name the same reachable set "
        f"({len(on_screen)} tagged marks on screen, {len(unreachable_per_wire)} "
        f"unreachable)",
        not (on_screen & unreachable_per_wire),
    )
    ok(
        f"E/{name}: and the comparison is not vacuous — the paint puts "
        f"{len(on_screen)} tagged marks on screen",
        len(on_screen) > 20,
    )
    if example != CONCEDING:
        return
    # ★★ R1713 — a conceded floor is expected to CLIP, and the count says so.
    # Without this the section passes for a band that gave up nothing, which is
    # the shape `stale` exists to catch on the other side.
    ok(
        f"E/{name}: the band really clips at the floor "
        f"({live.result['clipped']} mark(s) reachable in part only)",
        live.result["clipped"] > 0,
    )
    # ★★ The regions the policy gives up are CLIPPED, which is a different
    # statement from gone: each is still painted at the floor, and each is
    # still where a pointer finds it. A concession that blanked its regions —
    # or that left them drawn a band's width from where they can be pressed —
    # satisfies every structural check above.
    for tag in GIVES_UP:
        ok(f"E/{name}: {tag} is still painted at the conceded floor", tag in painted)
        x, y, w, h = painted[tag]
        ok(
            f"E/{name}: and the part of it the reader keeps is real "
            f"({w}x{h} at {x},{y})",
            w > 0 and h > 0 and x < size[0] and y < size[1],
        )
    # ★★★★★ R1700's question, asked at the size this round invented: every
    # painted rectangle is pressable where it is drawn. The band moves the
    # layout past the window's right edge, which is exactly the shape that
    # made 166 rectangles unpressable the last time a screen and its hit test
    # disagreed about a size.
    reports = assert_targets_survive_resize(app, [size], label=f"E/{name}")
    delivered = sum(
        s["deliverable"] + s["handle"] for s in reports[size]["surfaces"] if s["answers"]
    )
    ok(
        f"E/{name}: what is drawn is what is pressed at the conceded floor "
        f"({delivered} addressable)",
        delivered > 0,
    )


# ── F: rigid means it cuts nothing ──────────────────────────────────────────


def a_rigid_floor_cuts_nothing(name: str, report: dict) -> None:
    concession = report["concession"]
    assert_eq(
        concession["cut_at_floor"],
        [],
        f"F/{name}: a rigid floor shows every mark whole — which is the claim "
        f"`rigid` makes and a bare minimum size does not",
    )
    assert_eq(concession["covered"], 0, f"F/{name}: so there is nothing to cover")


# ── G: the declaration is one fact ──────────────────────────────────────────


def the_two_readers_read_one_declaration(name: str, report: dict) -> None:
    concession = report["concession"]
    ok(
        f"G/{name}: the floor the window system was told IS the policy's floor",
        concession["declaration_split"] is False,
    )
    declared = report["declared"]["floor"]
    assert_eq(
        (declared["width"], declared["height"]),
        (concession["floor"]["width"], concession["floor"]["height"]),
        f"G/{name}: and the two are published side by side so a reader can see it",
    )


# ── H: pixels ───────────────────────────────────────────────────────────────


def the_concession_is_visible_in_the_pixels(
    app: RpcSubprocess, name: str, size: tuple[int, int], png: Path
) -> None:
    app.request("scene/screenshot", {"path": "", "out_path": str(png)})
    assert png.exists(), f"H/{name}: no screenshot was written"
    img = read_png_rgba8(png)
    assert_eq(
        (img.width, img.height),
        size,
        f"H/{name}: the photograph is of the window at its conceded floor",
    )
    inked = inked_samples(img, 0, img.width)
    ok(f"H/{name}: and there is a screen in it ({inked} inked samples)", inked > 150)
    # ★ Scan where the concession IS. The band is the rightmost pixels of the
    # layout, which this window does not show; what a reader keeps is everything
    # left of it, and that is what the two halves compare.
    left = inked_samples(img, 0, size[0] // 2)
    right = inked_samples(img, size[0] // 2, size[0])
    ok(
        f"H/{name}: both halves of the clipped window still carry a screen "
        f"(left {left}, right {right})",
        left > 60 and right > 60,
    )
    print(f"[demo] H/{name}: {size[0]}x{size[1]}, {inked} inked samples -> {png.name}")


def inked_samples(img: Png, x0: int, x1: int) -> int:
    """Samples carrying ink in a column band — scanned, not glanced at."""
    inked = 0
    for row in range(4, img.height, 11):
        for col in range(x0 + 4, min(x1, img.width), 7):
            r, g, b, _ = png_pixel(img, col, row)
            if abs(r - g) > 6 or abs(g - b) > 6 or r > 60:
                inked += 1
    return inked


# ── the round ───────────────────────────────────────────────────────────────


def drive(name: str, example: str, tmp: Path) -> None:
    banner(f"{name} ({example})")
    with RpcSubprocess(example) as app:
        report = the_policy_is_declared_and_says_which_kind_it_is(app, name, example)
        what_is_clipped_is_what_was_declared(name, example, report)
        the_floor_is_where_reach_actually_ends(app, name, report)
        the_two_readers_read_one_declaration(name, report)
        if example != CONCEDING:
            a_rigid_floor_cuts_nothing(name, report)
        size = the_window_really_goes_there(app, name, report)
        what_is_given_up_is_still_reachable(app, name, example, size)
        if example == CONCEDING:
            the_concession_is_visible_in_the_pixels(app, name, size, tmp / f"{example}.png")


def main() -> None:
    with tempfile.TemporaryDirectory() as d:
        for name, example in SCREENS:
            drive(name, example, Path(d))
    print(f"\n{len(CHECKS)} assertions across {len(SCREENS)} screens")
    # A tripwire for "this file ran at all", not a coverage claim — the named
    # assertions above are the coverage. Kept low on purpose: it moved from 30
    # to 29 when the corrected floor stopped clipping the app bar's state chip,
    # and a tripwire that has to be re-tuned every time the screen changes is
    # one that will eventually be tuned instead of read.
    assert len(CHECKS) >= 20, f"only {len(CHECKS)} assertions"


if __name__ == "__main__":
    sys.exit(run_demo(Path(__file__).stem, main))
