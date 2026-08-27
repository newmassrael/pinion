#!/usr/bin/env python3
"""R1700 §5.15 §5.35 §5.20 §2 #7 — **what is drawn is what is pressed**, on all
three screens of the analysis tool, in real windows, at four sizes.

# What this exists for

A person reported the same thing twice about these screens: maximise the window
and controls stop responding. "R-01 will not press." "The row under the
timestamping field does not respond." Both times every gate was green, and both
times the cause was the same — the paint reflowed to the live window while the
hit test went on resolving against the size the screen was designed at.

Measured at the start of this round, driving a real shell at 2494x1011: on the
capture viewer, of the 166 painted rectangles that moved, **166** had stopped
being pressable where they were drawn. Nothing could see it, for two reasons
worth stating precisely:

* the in-process sweeps paint and hit-test inside ONE owner scope, where both
  halves resolve the size hook and therefore cannot disagree — the size axis is
  void by construction there;
* and the framework's own pointer guarantee (`scene/pointer_reach`) covers the
  REGISTERED widgets. §2 #7 makes a screen one `External`, so on that screen it
  vouched for **1 of 291** painted rectangles and the rest were on the screen's
  honour.

So the framework was given the missing question — `External::target_at` and
`target_of_tag`, compared against the paint by `scene/pointer_target` — and this
drives it through real windows.

# What it asserts

* **A** — the specification each screen publishes is on screen at every size:
  the panes, seats and rows it declares are painted, and a pane is painted
  between the width it can draw in and the width it is drawn at (R1860 — it was
  "a declared pane width is the width it gets", which is a claim about a
  constant once panes can flex).
* **B** — ★ the paint and the gesture agree, at every size. Every painted
  rectangle addressable by name is pressable at its own centre; every one that
  is not is honest decoration or honestly inert.
* **C** — a real press through the shell's router, aimed at a rectangle read out
  of the PAINTED scene at a maximised size, moves what the screen says. This is
  the half the person exercised and no gate did.
* **D** — the framework's new coverage is reported rather than assumed: how many
  of each screen's painted rectangles it can vouch for now, against the one it
  could before.

Floor, measured by building a probe at 6.11.1 and running it offscreen rather
than by reading documentation: a widget's own size answers live inside a press
handler there (1200x700 after a resize), which is the property `layout_size` had
to match. What that floor cannot do is any of B — a self-painting widget's eight
painted marks are invisible to the framework's point lookup, which answers null;
the scene-graph point lookup trusts an item's DECLARED shape and finds nothing
where a paint drew outside it; and no member enumerates what a widget painted,
because the only framework-held record of a paint there is pixels, which carry
no identity.

Run from the workspace root:
    cargo build -p hello-node-lab -p hello-packet-view -p hello-analyzer-shell --release
    python3 tools/demos/r1700_what_is_drawn_is_what_is_pressed.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_declared_panes_on_screen,
    assert_eq,
    assert_targets_survive_resize,
    behind_an_overflow,
    resize_and_settle,
    run_demo,
)

EXT = "/external"

# ★ The two large sizes are the ones a person actually had on screen when they
# reported this, read off their window manager rather than invented here; the
# small one is a window narrower than every screen's design width and taller
# than one of them, which is the case a screen with a layout floor answers
# differently.
BIGGER = [(2494, 1011), (2494, 1531), (1200, 1080)]


def sizes_for(app: RpcSubprocess) -> list[tuple[int, int]]:
    """This screen's own design size, then the three to resize it to.

    ★ The design size is ASKED FOR rather than written down, and the first draft
    of this file wrote down 1440x900. The node lab opens at 1625x900 — its panes
    do not fit in less — so the constant was resizing that screen BELOW its
    layout floor and calling the result its design size, which reported it
    vouching for 54 rectangles where it vouches for 61. A number that has to
    track a per-screen fact is not a constant, which is the same sentence this
    round is about one level up.
    """
    rect = app.snapshot(source="paint")["rect"]
    return [(rect["w"], rect["h"]), *BIGGER]

#: ★★★★★ What each screen could answer by name when this was measured, at the
#: design size, and a FLOOR rather than a report.
#:
#: It is here because a counterfactual PASSED without it. Make a screen's
#: by-name answer drift from the tags its painter emits — trim the ordinal off
#: every indexed tag, which is the exact way a hand-written inverse rots — and
#: every affected rectangle stops being `deliverable` and becomes `covering`,
#: which is a LEGITIMATE verdict (a label over the row it labels) and passes.
#: The census publishes the collapse as a number; nothing read the number.
#:
#: So a screen that quietly stops vouching for two thirds of itself now fails.
#: Raise these when a screen answers for more; a drop is the round's to explain.
COVERAGE_FLOOR = {
    "capture viewer": 224,
    "node lab": 61,
    "shell": 28,
}

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def rects(app: RpcSubprocess, size: tuple[int, int]) -> dict:
    return abs_rects_of(app.snapshot(source="paint", viewport=size))


def targets(app: RpcSubprocess) -> dict:
    resp = app.request("scene/pointer_target")
    assert resp is not None and resp.result is not None
    return resp.result


def answering(report: dict) -> list[dict]:
    return [s for s in report["surfaces"] if s["answers"]]


# ── A: the published specification is on screen, at every size ──────────────


def spec_is_on_screen(app: RpcSubprocess, name: str, sizes: list) -> None:
    """Every pane the screen DECLARES is painted, and gets the width it claims.

    Read out of the screen's own `spec`, never written down here: a pane that is
    renamed or re-sized in the specification moves this with it, and a pane
    added to the specification and not to the painter fails.
    """
    # ★ Two spellings again: one screen declares `spec` as `json` and another as
    # a `string` holding json. Read both rather than picking one, because which
    # a screen chose is not what this is about.
    spec = app.query(f"{EXT}/spec")
    if isinstance(spec, str):
        spec = json.loads(spec)
    panes = spec.get("panes") if isinstance(spec, dict) else None
    if not panes:
        print(f"[demo] A/{name}: the specification is not organised in panes")
        return
    elastic = [p["tag"] for p in panes if not p["width"]]
    widths: dict[tuple[int, int], int] = {}
    for size in sizes:
        resize_and_settle(app, size)
        painted = rects(app, size)
        # ★★ R1714 — the per-size half is the SHARED rule now, not a second copy
        # of it. This file and `assert_declared_panes_on_screen` had been
        # asserting the same three things about panes since R1700 wrote them
        # here; when a window that pans made "every declared pane is painted"
        # too strong, both had to learn the same repair, and writing it twice is
        # how the two would have parted. What stays here is the claim only this
        # file makes: what a WIDER window gives its room to.
        assert_declared_panes_on_screen(app, size, label=f"A/{name}")
        if len(elastic) == 1 and elastic[0] in painted:
            widths[size] = painted[elastic[0]][2]
    # ★ And the elastic pane is what a wider window gives its room to, which is
    # the claim a fixed-width declaration is only half of.
    if len(elastic) == 1:
        design, wide = sizes[0], sizes[1]
        assert_eq(
            widths[wide] > widths[design],
            True,
            f"A/{name}: the elastic pane takes the room a wider window adds "
            f"({widths[design]} at {design[0]}, {widths[wide]} at {wide[0]})",
        )
    print(f"[demo] A/{name}: {len(panes)} declared pane(s) checked at {len(sizes)} size(s)")


def named_in_the_spec(spec: object) -> set[str]:
    """Every string anywhere in a published specification.

    The three screens organise their specifications differently — panes and
    columns here, a rail roster and a catalogue there — so the general form
    reads all of it rather than knowing any of it.
    """
    if isinstance(spec, str):
        return {spec}
    if isinstance(spec, dict):
        return set().union(*(named_in_the_spec(v) for v in spec.values())) if spec else set()
    if isinstance(spec, list):
        return set().union(*(named_in_the_spec(v) for v in spec)) if spec else set()
    return set()


def what_the_spec_names_stays_on_screen(app: RpcSubprocess, name: str, sizes: list) -> None:
    """★ Whatever a screen's specification names AND paints, it goes on painting
    at every window size.

    The general half of A, and the one that reaches the screen whose
    specification is not organised in panes. The population is the intersection
    of "named in the specification" with "painted at the design size", which is
    what makes it self-calibrating across three differently-shaped
    specifications — and non-vacuous, because the intersection is asserted
    non-empty rather than allowed to be the reason nothing was checked.

    What it catches is a declared thing that survives at the size it was
    designed at and vanishes at another, which is the same class as the defect
    this round repaired seen from the paint's side rather than the gesture's.
    """
    spec = app.query(f"{EXT}/spec")
    if isinstance(spec, str):
        spec = json.loads(spec)
    design = sizes[0]
    resize_and_settle(app, design)
    declared = named_in_the_spec(spec) & set(rects(app, design))
    ok(f"A2/{name}: the specification names things that are on screen", len(declared) >= 8)
    for size in sizes[1:]:
        resize_and_settle(app, size)
        gone = sorted(declared - set(rects(app, size)))
        # ★★★★★ R1714 — painted, **or one gesture away**, the same repair the
        # pane check next door took and for the same reason: a window whose
        # policy declares a pan is a viewport onto a layout bigger than itself,
        # so at 1200 wide the node lab's whole inspector is off screen and one
        # scroll from being on it. Measured: 12 declared regions there.
        #
        # The class this check exists for is untouched — a declared thing that
        # is neither drawn nor reachable still fails, by name.
        if gone:
            reach = app.request("scene/scroll_reach")
            assert reach is not None and isinstance(reach.result, dict)
            reachable = {
                row["tag"]
                for row in reach.result["out_of_sight"]
                if row["reach"] == "scrollable" and row["tag"]
            }
            gone = [tag for tag in gone if tag not in reachable]
        # ★★★★★ R1795 — and a control the TOOLBAR gave up is reachable too, by a
        # press rather than a scroll. R1791 let a row move a group behind an
        # overflow control when it is tight, so at 1200 wide the node lab's zoom
        # group is one press away and not gone. Asked of the screen, because
        # what is behind the control at one width is on the row at another and
        # no caller can compute it. This is the second demo to need the
        # subtraction and the first to learn it from CI — `r1709` took it in the
        # round that caused it, and nothing pointed at this one.
        gone = [tag for tag in gone if tag not in behind_an_overflow(app)]
        assert_eq(
            gone,
            [],
            f"A2/{name} {size}: everything the specification names is still "
            f"painted or reachable",
        )
    print(f"[demo] A2/{name}: {len(declared)} declared-and-painted tag(s) held at every size")


# ── B: the paint and the gesture agree, at every size ───────────────────────


def paint_and_gesture_agree(app: RpcSubprocess, name: str, sizes: list) -> dict:
    reports = assert_targets_survive_resize(app, sizes, label=name)
    for size, report in reports.items():
        surfaces = answering(report)
        ok(f"B/{name} {size}: a surface answers what is under a press", bool(surfaces))
        assert_eq(report["defects"], 0, f"B/{name} {size}: nothing disagrees with its own paint")
    # ★ And the count of rectangles the framework can vouch for must not
    # collapse when the window grows. Without this the check above is satisfied
    # by a screen that stops answering — which is precisely the failure this
    # round repaired, and it would otherwise read as "no disagreements".
    # ★★★★★ R1795 — plus what the toolbar's overflow control is HOLDING. Since
    # R1791 a row can give a group up when it is tight, so this count stopped
    # being a property of the screen and became a property of the window — the
    # same thing `r1651`'s family roster learned in that round, with the same
    # repair.
    #
    # Measured before changing anything, because the floor's own rule is that a
    # drop is the round's to explain: the node lab answers for **59** at its
    # design width with `export` and `file` moved, and **63** at 1700 and 2000
    # where nothing moves. It did not stop vouching for part of itself — it
    # vouches for MORE than the committed 61 whenever the row is whole, and five
    # of its seats are one press away rather than on the row at the narrow size.
    held = len(behind_an_overflow(app))
    base = sum(s["deliverable"] + s["handle"] for s in answering(reports[sizes[0]])) + held
    assert_eq(
        base >= COVERAGE_FLOOR[name],
        True,
        f"B/{name}: the screen answers by name for {base} painted rectangle(s), "
        f"and the committed floor is {COVERAGE_FLOOR[name]} — a drop means it "
        f"has stopped vouching for part of itself, which reads as `covering` "
        f"and would otherwise pass",
    )
    for size, report in reports.items():
        got = sum(s["deliverable"] + s["handle"] for s in answering(report))
        ok(
            f"B/{name} {size}: {got} rectangle(s) are pressable where drawn "
            f"(the design size has {base})",
            got >= base // 2,
        )
    return reports


# ── C: a real press, aimed where the paint put it, at a maximised size ──────


def cursor_of(app: RpcSubprocess) -> tuple[int, int]:
    """Where the surface says the pointer last reached, in its OWN pixels.

    All three screens publish this, in two spellings — a `"x,y"` string and an
    `{x, y}` object — which is itself worth a line: the vocabulary is per screen
    and the fact is not.
    """
    said = app.query(f"{EXT}/cursor")
    if isinstance(said, dict):
        return (int(said["x"]), int(said["y"]))
    x, y = str(said).split(",")
    return (int(x), int(y))


def a_press_lands_where_it_is_drawn(app: RpcSubprocess, name: str, sizes: list) -> None:
    """★ The FRAMEWORK half: a press aimed at a painted rectangle arrives at the
    surface at the pixel it was aimed at, in a maximised window.

    `paint_and_gesture_agree` above proves the screen agrees with itself. What
    it cannot prove is the chain between them — the shell's router turns a
    window pixel into a fraction of the surface's rectangle and the surface
    multiplies that fraction back out by the size it believes it is. That
    arithmetic is precisely what was wrong, and its error grows with distance
    from the origin, which is why the far right of the screen died first and the
    left pane went on working.

    So the aim comes out of the PAINTED scene, the press goes through
    `scene/click` (winit's event arc into the shell's router — a mouse's path,
    and one no in-process fixture runs), and the witness is the surface's own
    published cursor. Nothing here is derived from the hit test, so this cannot
    pass by the screen agreeing with a copy of itself.

    ★★★★★ R1737 — **the tolerance is gone, and it was the size of a defect.**

    This used to allow one pixel either way, on the stated grounds that "the
    router divides to a float and the surface multiplies back, so the round trip
    is genuinely inexact". That was true when it was written and it is exactly
    the error R1736 then found a person reporting: a press delivered one pixel
    left or up, at some coordinates and not others, which on a nine-pixel pin is
    an eighth of the target. The allowance could not have caught it.

    `pinion_core::external::pixel_of` makes the round trip exact — asserted over
    the whole range of five extents, and measured over 6,015 real-pointer
    arrivals on five screens with zero drift (R1737) — so a tolerance calibrated
    to the old inexactness is now a blindfold rather than a kindness.
    """
    big = sizes[1]
    resize_and_settle(app, big)
    report = targets(app)
    surface = answering(report)[0]["surface"]
    origin = rects(app, big)[surface]
    rows = [row for row in answering(report)[0]["rows"] if row["verdict"] == "deliverable"]
    ok(f"C/{name}: there are rectangles to press at {big}", len(rows) >= 8)
    # A stable, spread sample rather than all of them: each is a round trip and
    # what is under test is the arithmetic, which does not vary by row.
    sample = rows[:: max(1, len(rows) // 12)][:12]
    wrong = []
    for row in sample:
        app.click((row["x"], row["y"]))
        app.tick(16)
        want = (row["x"] - origin[0], row["y"] - origin[1])
        got = cursor_of(app)
        if got != want:
            wrong.append(f"{row['tag']} aimed at {want} and arrived at {got}")
    assert_eq(wrong, [], f"C/{name}: a real press arrives where the paint put it")
    print(f"[demo] C/{name}: {len(sample)} real press(es) at {big[0]}x{big[1]}, all landed")


# ── D: what the framework can vouch for now ─────────────────────────────────


def coverage(app: RpcSubprocess, name: str, reports: dict, sizes: list) -> None:
    report = reports[sizes[1]]
    reach = app.request("scene/pointer_reach").result
    for surface in answering(report):
        print(
            f"[demo] D/{name}: {surface['surface']} — "
            f"{surface['deliverable']} deliverable, {surface['covering']} covering, "
            f"{surface['inert']} inert of {surface['painted']} painted; "
            f"`pointer_reach` alone vouches for {reach.get('deliverable', 0)}"
        )
        ok(
            f"D/{name}: the framework vouches for more than the surface itself",
            surface["deliverable"] + surface["covering"] + surface["inert"]
            > reach.get("deliverable", 0),
        )


# ── the three screens ───────────────────────────────────────────────────────


def screen(example: str, name: str) -> None:
    banner(f"{name} ({example})")
    with RpcSubprocess(example) as app:
        sizes = sizes_for(app)
        print(f"[demo] {name}: design size {sizes[0]}, then {sizes[1:]}")
        spec_is_on_screen(app, name, sizes)
        what_the_spec_names_stays_on_screen(app, name, sizes)
        reports = paint_and_gesture_agree(app, name, sizes)
        a_press_lands_where_it_is_drawn(app, name, sizes)
        coverage(app, name, reports, sizes)


def body() -> None:
    # ★ The capture viewer first: it is the screen the defect was on, so a
    # regression shows up before anything else has run.
    screen("hello-packet-view", "capture viewer")
    screen("hello-node-lab", "node lab")
    screen("hello-analyzer-shell", "shell")
    print(f"\n[demo] {len(CHECKS)} named check(s) across three screens")


if __name__ == "__main__":
    run_demo("r1700 what is drawn is what is pressed", body)
