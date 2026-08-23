#!/usr/bin/env python3
"""R1709 §5.16 §5.12 §2 #7 §2 #2 — **a resize is followed by pixels**, on all
three screens of the analysis tool, in the HIDDEN mode every demo and all of CI
runs in.

# What this exists for

R1708 found, while doing something else, that resizing a window that was never
mapped left it unable to present ever again: one resize took `present_ok` from
`true` to `false`, `scene/screenshot` answered `RenderBackendUnavailable` on
every attempt afterwards, and a second resize did not recover it either. Eight
consecutive dead frames, permanently.

Nothing in the tree had noticed, and the reason is structural rather than
careless: **of the sixteen demos that take screenshots, none also resized**.
`scene/snapshot {from:"paint"}` reads the ENCODED scene, which is built before
the swapchain is ever asked for an image, so every introspection surface goes on
answering as though the screen were fine. This tree had never once checked what
is on the screen AFTER a resize.

So the point of this file is the question no file was asking.

# What was measured, and what turned out to be true

The debt entry's hypothesis was that the surface was configured for a size the
X drawable never took. Measured, all three parties agreed — X said 1100x700,
the window said 1100x700, the swapchain was configured 1100x700 — and it was
still dead. Isolated in a standalone reproducer with no framework in it at all:

* a frame after the window resized, WITHOUT touching the surface: **presents**;
* `configure()` to the new size, then frames: **outdated, for ever**;
* drop the surface, make a new one for the same window: **presents**.

So the reconfigure is what poisons that swapchain, and it is exactly what the
tree ran — once per failed frame, for ever, under a comment asserting that it
"re-establishes the swapchain so the NEXT frame acquires a fresh texture".
Nothing had ever checked whether the next frame did.

★ It is one cell of four: hidden on a real X server with a window manager. A
mapped window is fine, and so is either on a bare offscreen server. That is why
this gate drives the hidden path — the one three of the four cells cannot see.

# What it asserts

* **A** — the analyser's published specification is on screen before anything
  is resized: every pane each screen DECLARES is painted, at the width it
  declares, tiling the body. Read out of the screen's own spec, never written
  down here.
* **B** — ★★ the gate that was missing. At every size, a screenshot comes back,
  it is the size the window now is, and it is PAINTED — scanned, not glanced
  at. Before R1709 this section failed on the first resize and stayed failed.
* **C** — the specification survives the resize: everything the screen names
  and paints at its design size is still painted at every other size.
* **D** — ★ the recovery ladder is published and coherent. `presenting` agrees
  with `missed_in_a_row`, breakages never outnumber misses, and a reason and a
  rung are present exactly when something is broken.
* **E** — every `last_missed` and `last_rung` this run produces is a member of
  the declared roster. ★★ Reported, NOT counted on, and the honest reading is
  that it observes nothing at all: a reason is published only while something
  is broken, and since the ladder now recovers inside the same frame, a reader
  arriving afterwards finds nothing to name — on the affected host as much as
  on a healthy one (measured: zero observations on both, while `rebuilds` says
  the heavy rung ran three times). So the assertion that CAN always fire lives
  in Rust instead, where `pinion_rpc::render_fidelity`'s tests build a dark
  window's record directly. This section keeps its clauses because a value
  outside the roster would still be caught, and `main` prints the observation
  count so a quiet run cannot be mistaken for a proved one.
* **F** — ★ the pixels are of THIS window, not a stale one: the screenshot at a
  narrow size and at a wide size differ in the band only the wide one has.
  Without this, a surface that kept presenting the pre-resize image would pass
  B (right dimensions, plenty of ink) and be exactly the defect.
* **H** — ★★ the window comes back with nothing capturing it. A capture is
  itself a present, so every other section here silently repairs the very thing
  it is checking; this one only READS, and waits for `presenting` to come true
  on its own.

★★★★★ H exists because counterfactuals PASSED, and it is the section that
changed the round's design. Under the first draft's order — resize, settle,
screenshot — three of four repairs could each be deleted with every assertion
still green, because a capture is a present and healed whatever was broken
before anything looked at it. Adding H then falsified two of those repairs
outright: the "ask for another frame" one never fired at all (`paint_seq`
stuck for 3.2 s with a rung recorded and no frame following), and the capture
path's climb turned out to spend the ladder's heavy rung on a window that was
merely mid-resize. Both were deleted, and the ladder now runs where it belongs
— in the frame that just reconfigured the surface for the size the window
actually has.

★ Every one of these is falsifiable on a host where the driver defect does not
occur: B and F fail if a resize stops producing pixels for ANY reason, C fails
if a resized screen loses a declared region, D and E fail on an incoherent or
unnamed publication. The round did not want a gate that can only fire on one
GPU.
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    Png,
    RpcSubprocess,
    assert_declared_panes_on_screen,
    assert_eq,
    declared_and_painted,
    settled_baseline,
    design_size,
    png_pixel,
    read_png_rgba8,
    resize_and_settle,
    run_demo,
    wait_until,
)

#: The three screens of the tool, in the order the specification names them.
SCREENS = [
    ("node lab", "hello-node-lab"),
    ("capture viewer", "hello-packet-view"),
    ("shell", "hello-analyzer-shell"),
]

#: Sizes to drive, relative to whatever the screen opens at. Both directions,
#: because a shrink and a grow are different requests to the window system and
#: the defect this gate exists for was found on a shrink.
DELTAS = [(+180, +90), (+60, +40)]

#: Names the framework publishes for why a frame missed the screen, and for the
#: rung of the recovery ladder it earned. Asserted against rather than merely
#: printed: a value outside these is a vocabulary that reached the wire without
#: being named, which is how a census stops being able to see it.
MISSED_NAMES = {"outdated", "lost", "validation", "timeout", "occluded"}
RUNG_NAMES = {"reconfigured", "rebuilt", "repeated"}

#: ★★★★★ R1711 — R1710's `FLOOR_LOSS` table lived here, naming five regions of
#: the node lab as measurably lost at its own declared floor. Measured again
#: through `scene/scroll_reach`: **all five are `scrollable`**, each with the
#: offset that shows it, and the screen's `lost` count at that size is zero. The
#: five were not lost; they were below the fold of two scrolling panes, and the
#: question "is it painted right now" cannot tell those apart.
#:
#: So the table is gone and the check below asks the question that has an
#: answer: a declared region that is not painted at this size must be one the
#: reader can SCROLL to, and nothing may be lost. That is strictly stronger — it
#: fails on a region that vanishes for any other reason — and it needs no
#: per-screen exemptions at all.

CHECKS: list[str] = []

#: How many times this run actually SAW a published reason / rung. Printed at
#: the end because sections D and E can only judge what a host produces, and a
#: run that observed none has not proved the naming — it has proved nothing
#: broke. Saying so is the difference between a quiet gate and a vacuous one.
OBSERVED: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    assert condition, what
    CHECKS.append(what)


def fidelity(app: RpcSubprocess) -> dict:
    resp = app.request("scene/render_fidelity")
    assert resp is not None and resp.result is not None, "scene/render_fidelity answers"
    return resp.result


def shoot(app: RpcSubprocess, png: Path) -> Png:
    """A screenshot, through the live-surface path — the one that goes to the
    swapchain and therefore the one that can be dead."""
    app.request("scene/screenshot", {"path": "", "out_path": str(png)})
    assert png.exists(), "the screenshot was not written"
    return read_png_rgba8(png)


def inked_in_band(img: Png, x0: int, x1: int) -> int:
    """Samples carrying ink in the column band `[x0, x1)`, over the whole height.

    ★ SCANNED rather than looked at, and over the whole height rather than one
    row: a single row can cross a pane of uniform background and answer 1 on a
    perfectly painted window.
    """
    inked = 0
    for row in range(4, img.height, 11):
        for col in range(x0, min(x1, img.width), 5):
            r, g, b, _ = png_pixel(img, col, row)
            if abs(r - g) > 6 or abs(g - b) > 6 or r > 60:
                inked += 1
    return inked


# ── D / E: what the window says about presenting ────────────────────────────


def the_ladder_is_coherent(app: RpcSubprocess, name: str, when: str) -> dict:
    """The published presentability is internally consistent.

    None of this can pass vacuously: `health` is asserted present, and each
    clause is a relation between two published numbers rather than a
    restatement of one.
    """
    report = fidelity(app)
    health = report.get("health")
    ok(f"D/{name}/{when}: the frame record publishes presentability", isinstance(health, dict))
    missed = health["missed_in_a_row"]
    broken = health["broken_in_a_row"]
    ok(
        f"D/{name}/{when}: `presenting` agrees with the miss count "
        f"(presenting={health['presenting']}, missed={missed})",
        health["presenting"] == (missed == 0),
    )
    ok(
        f"D/{name}/{when}: breakages are a subset of misses ({broken} <= {missed})",
        broken <= missed,
    )
    # A reason and a rung appear exactly when there is something to explain.
    # Both directions, because "always absent" and "always present" are each
    # satisfiable by a publication that is not reading the state at all.
    ok(
        f"D/{name}/{when}: a miss is published with its reason",
        (health.get("last_missed") is not None) == (missed > 0),
    )
    ok(
        f"D/{name}/{when}: a rung is published exactly when the surface broke",
        (health.get("last_rung") is not None) == (broken > 0),
    )
    ok(
        f"D/{name}/{when}: the rebuild count is a count ({health['rebuilds']})",
        isinstance(health["rebuilds"], int) and health["rebuilds"] >= 0,
    )
    # E — the vocabulary is the declared one, for whatever this host produced.
    reason, rung = health.get("last_missed"), health.get("last_rung")
    ok(
        f"E/{name}/{when}: the published reason is a named one ({reason!r})",
        reason is None or reason in MISSED_NAMES,
    )
    ok(
        f"E/{name}/{when}: the published rung is a named one ({rung!r})",
        rung is None or rung in RUNG_NAMES,
    )
    if reason is not None or rung is not None:
        OBSERVED.append(f"{name}/{when}: {reason}/{rung}")
    return health


# ── H: what a screenshotting demo cannot see ────────────────────────────────
#
# ★★★★★ THIS SECTION EXISTS BECAUSE COUNTERFACTUALS PASSED, and adding it then
# refuted two of the round's own repairs. The first draft of this file drove
# `resize -> settle -> screenshot`, and under that order three of four repairs
# could each be deleted with every assertion still green — because a capture IS
# a present, so whichever repair remained healed the surface before anything
# looked at it. H only READS, which is what made the difference visible.


# A section G was written here and DELETED, and what it was for is worth
# keeping: "an agent that resizes and immediately asks for a picture gets one".
# Measured, that premise is false and should be — a capture arriving before the
# resize has landed finds the swapchain legitimately stale, and the only way to
# make it succeed is to let one request REBUILD the surface, which spends the
# ladder's heavy rung on a window that was about to be fine anyway. So the
# capture path takes one rung and reports, the next frame's ladder restores the
# window, and this file asserts that recovery in H instead.


def the_window_recovers_without_being_photographed(app: RpcSubprocess, name: str) -> None:
    """H — the window comes back on its own, with nothing capturing.

    ★★ A capture is itself a present, so it settles the recovery ladder. Every
    other section here takes screenshots, which means every other section
    silently repairs the very thing it is checking — measured: with the
    renderer's own "a presented frame settles the ladder" deleted, and again
    with "a frame that missed asks for another one" deleted, this file stayed
    green from end to end. A real application never screenshots itself.

    So this waits on `presenting` while doing nothing but READ. Reads do not
    present, so the only thing that can turn it true is the shell recovering by
    itself. A wait rather than an instantaneous read because the recovery lands
    on a later frame and asserting on the first one would be a race — the
    zero-flake rule, and the reason this can be both deterministic and
    falsifiable.
    """
    wait_until(
        lambda: fidelity(app)["health"]["presenting"],
        timeout=15.0,
        desc=f"H/{name}: the window presents again with nothing capturing it",
    )
    CHECKS.append(f"H/{name}: the window recovered without being photographed")


# ── the round ───────────────────────────────────────────────────────────────


def drive(name: str, example: str) -> None:
    banner(f"{name} ({example})")
    # ★★ HIDDEN — the harness default, and the whole point. Every demo in this
    # tree and every job in CI runs a window that was never mapped, and that is
    # the one configuration in which the defect this gate exists for occurs. A
    # gate that mapped the window to be comfortable would be testing the path
    # that already worked.
    with RpcSubprocess(example) as app:
        design = design_size(app)

        # A — the specification is on screen before anything moves.
        made = assert_declared_panes_on_screen(app, design, label=f"A/{name}")
        if made:
            CHECKS.extend(made)
        else:
            print(f"[demo] A/{name}: the specification is not organised in panes")
        # R1790 — settled, because this baseline is compared against a LATER
        # read and a region with a lifetime is not a stable member of one. The
        # shell says `Overview loaded` at boot, so `shell.toast` was in here and
        # gone by the comparison on a slow runner; R1787's CI run failed exactly
        # that way while this demo passed locally in 8 seconds.
        declared = settled_baseline(app, design)
        ok(
            f"A/{name}: the specification names things that are on screen ({len(declared)})",
            len(declared) >= 8,
        )
        the_ladder_is_coherent(app, name, "before")

        with tempfile.TemporaryDirectory() as d:
            tmp = Path(d)
            first = shoot(app, tmp / f"{example}-design.png")
            ok(
                f"B/{name}: the opening window can be photographed "
                f"({first.width}x{first.height})",
                (first.width, first.height) == design,
            )
            narrow_img: Png | None = None
            widest = 0
            widest_img: Png | None = None

            # ★★★★★ R1710 — the SMALLEST LEGAL window first, read out of the
            # screen's own declaration instead of guessed at. This list used to
            # open with `design - (525, 200)`, which on every one of these three
            # screens is BELOW the floor the binding declares: the node lab's own
            # source complains that "every headless probe laid the screen out at
            # a width the screen says it does not support", and this was one of
            # the probes doing it. It passed because the bare display CI runs on
            # enforces no declared minimum — a window manager always did.
            #
            # A dry-run ask of 1x1 comes back as the floor (§2 #3), so the demo
            # drives a genuinely small window where one is legal (the node lab
            # shrinks to 360 tall) and the design size where it is not.
            floor_resp = app.request(
                "scene/resize", {"width": 1, "height": 1, "dry_run": True}
            )
            floor = (floor_resp.result["width"], floor_resp.result["height"])
            sizes = [floor] + [(design[0] + dw, design[1] + dh) for dw, dh in DELTAS]
            ok(
                f"B/{name}: the three sizes driven are distinct ({sizes})",
                len(set(sizes)) == len(sizes),
            )

            for i, size in enumerate(sizes):
                resize_and_settle(app, size)

                # H FIRST, before anything in this iteration captures: a
                # capture presents, and a presented frame settles the ladder,
                # so a screenshot taken here would repair what H checks.
                the_window_recovers_without_being_photographed(app, name)

                # ★★ B — THE assertion of this file. Before R1709 the request
                # below raised `RenderBackendUnavailable` here, on the first
                # iteration, and every iteration after it.
                img = shoot(app, tmp / f"{example}-{i}.png")
                ok(
                    f"B/{name} {size}: a resize is followed by pixels "
                    f"({img.width}x{img.height})",
                    (img.width, img.height) == size,
                )
                painted = inked_in_band(img, 0, img.width)
                ok(
                    f"B/{name} {size}: and they are painted ({painted} inked samples)",
                    painted > 200,
                )

                # C — the screen is still itself at this size.
                #
                # ★★★★★ R1711 — the predicate, corrected. R1710 asked whether
                # every declared region was still PAINTED and read five regions
                # of the node lab as lost at its own floor. They are not lost:
                # they are below the fold of two scrolling panes, and the read
                # that can tell those apart says so. So the question here is now
                # "is anything unreachable", which needs no exemption table.
                #
                # ★ R1713 — `scrollable` is no longer the only answer that means
                # "the reader can get to this". A region the range brings all but
                # an edge of now answers `clipped`, which is what a conceded floor
                # buys and is not what this section is looking for; `lost` below
                # is the bar, and it is unchanged. Measured when the arm landed:
                # the node lab's inspector note is `clipped` at its conceded
                # floor and was `scrollable` before, so this read failed on a
                # region the reader can still scroll to.
                gone = sorted(declared - declared_and_painted(app, size))
                reach = app.request("scene/scroll_reach")
                assert reach is not None and reach.result is not None
                rows = reach.result["out_of_sight"]
                reachable = {
                    row["tag"]
                    for row in rows
                    if row["reach"] in ("scrollable", "clipped") and row["tag"]
                }
                assert_eq(
                    [g for g in gone if g not in reachable],
                    [],
                    f"C/{name}: every declared region {size} does not paint is one "
                    f"the reader can bring into view",
                )
                assert_eq(
                    reach.result["lost"],
                    0,
                    f"C/{name}: nothing on this screen is out of reach at {size}",
                )
                CHECKS.append(
                    f"C/{name} {size}: {len(declared)} declared regions survive "
                    f"({len(gone)} of them by scrolling)"
                )

                health = the_ladder_is_coherent(app, name, f"at {size}")
                print(
                    f"[demo] {name} {size}: {painted} inked, presenting="
                    f"{health['presenting']}, rebuilds={health['rebuilds']}"
                )
                if i == 0:
                    narrow_img = img
                if size[0] > widest:
                    widest, widest_img = size[0], img

            # F — the pixels are of THIS window. A surface that went on
            # presenting the pre-resize image would satisfy B at every size.
            assert narrow_img is not None and widest_img is not None
            added = inked_in_band(widest_img, narrow_img.width, widest_img.width)
            ok(
                f"F/{name}: the columns only the wider window has are painted "
                f"({added} inked in x=[{narrow_img.width},{widest_img.width}))",
                added > 20,
            )
            # ...and the two are not the same picture, which is the cheapest
            # possible statement that something actually changed.
            ok(
                f"F/{name}: the narrow and wide frames are different pictures",
                narrow_img.width != widest_img.width,
            )

        # And the window is presenting at the end, which is the plain form of
        # what this whole file is about.
        end = fidelity(app)
        ok(
            f"B/{name}: the window is still presenting after {len(DELTAS)} resizes",
            end["health"]["presenting"] is True,
        )
        # ★ On a host where the heavy rung was needed, the claim is sharper and
        # this is where it is made: the surface was remade — repeatedly — and
        # the window is presenting anyway. Guarded rather than asserted
        # unconditionally, because "the ladder was never needed" is the correct
        # outcome on a host without the driver defect and must not read as a
        # failure. Both branches are recorded, so the log says which host this
        # was.
        rebuilds = end["health"]["rebuilds"]
        if rebuilds:
            ok(
                f"B/{name}: the surface was remade {rebuilds}x and the window "
                f"presents anyway",
                end["health"]["presenting"] is True and end["health"]["missed_in_a_row"] == 0,
            )
        else:
            CHECKS.append(f"B/{name}: the ladder's heavy rung was never needed on this host")


def main() -> None:
    for name, example in SCREENS:
        drive(name, example)
    print(f"\n{len(CHECKS)} assertions across {len(SCREENS)} screens")
    # ★ Say what sections D and E actually got to judge. A run with zero
    # observations has proved that nothing broke, which is a fine outcome and
    # not the same thing as having proved the naming.
    if OBSERVED:
        print(f"[demo] E: {len(OBSERVED)} published reason/rung observation(s):")
        for line in OBSERVED:
            print(f"[demo]    {line}")
    else:
        print(
            "[demo] E: nothing broke on this host, so no reason or rung was "
            "published — the naming is proved in pinion_rpc's own tests, not here"
        )
    assert len(CHECKS) >= 40, f"only {len(CHECKS)} assertions"


if __name__ == "__main__":
    run_demo("hello-node-lab", main)
