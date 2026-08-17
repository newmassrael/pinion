#!/usr/bin/env python3
"""R1711 §5.16 §5.32 §5.12 §2 #3 §2 #7 — **a screen says how small it can get,
and what stops it going smaller**, on all three screens of the analysis tool.

# What this exists for

Every binding in this tree declares a floor for its window — 225 of them, and
since R1710 the framework enforces it. Not one of those numbers had ever been
checked against the screen it is about. R1710 tried, with the predicate a reader
reaches for first (*is every declared region still painted?*) and reported five
regions of the node lab lost at its own floor. Measured again here through
`scene/scroll_reach`: **all five are `scrollable`**, each with the offset that
shows it, and the screen's `lost` count there is zero. The screen was fine; the
question was wrong, and section **F** drives the offsets to prove it.

The right predicate is sharper in the other direction, and this file measured
why. At 1506 pixels wide the node lab reports nothing out of sight — because one
pixel of a 100-pixel status chip is still on screen and its 312-pixel inspector
pane still starts at 1313. A floor derived from *that* is a floor at which the
inspector is sliced. So `scene/size_floor` judges through
`pinion_core::reach::cut`: what can this size never show **whole**.

# What it asserts

Nothing here writes a screen's floor down. Section **A** measures it and every
later section is computed from what it found, so one file drives three screens
whose floors genuinely differ.

* **A** — the floor is measured, and it agrees with what the binding declares
  (`verdict: exact`). This is the assertion the whole round is for: two numbers
  about one screen, from two places, checked against each other. It is also
  what caught the capture viewer declaring `Fixed` — a window a reader could
  not shrink at all, on a screen measured to work 554 pixels shorter.
* **B** — ★ the number carries its evidence: each axis names marks that go out
  of reach one pixel below it, and the boundary is real in BOTH directions —
  asked about `extent` nothing is cut, asked about `short_extent` the named
  marks are, and both asks are answered without the window moving (§2 #3).
* **C** — the pair of the two axis answers is checked, not assumed. Each axis is
  measured with the other at the ceiling, so their pair is a third question; on
  this tool it answered `loses` on two of three screens until the framework's
  per-axis clamp landed, and the field is what keeps that visible.
* **D** — the floor is real in the window: resize to it, and the granted size,
  the painted rectangle and the specification all agree, with nothing lost.
* **E** — §2 #3: the dry answer and the live one are the same fact. Asking about
  the size the window already is answers exactly what the live read answers, and
  asking about another size does not move the window.
* **F** — ★★ R1710's five names, driven: at the node lab's floor each is
  reported `scrollable` with an offset, and scrolling there makes it reachable
  in the paint. A read that says "one scroll away" is a claim about a gesture,
  so the gesture is what checks it.
* **G** — a malformed ask is refused BY NAME (`InvalidAt`) rather than quietly
  answered about the live window, which is what this method did before R1711.
* **H** — ★ pixels. At its measured floor each screen is photographed and
  scanned, because every section above reads structure and a floor that paints
  nothing would satisfy all of them.
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    Png,
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_declared_panes_on_screen,
    assert_eq,
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

#: The five regions R1710 recorded as lost at the node lab's own floor, and
#: filed as a defect of the screen. Kept HERE, by name, because section F drives
#: every one of them: the claim being checked is no longer "these are missing"
#: but "these are one scroll away, and the scroll works".
R1710_FIVE = [
    "lab.inspector.note",
    "lab.inspector.note.text",
    "lab.palette.discovery",
    "lab.palette.discovery.state",
    "lab.palette.discovery.track",
]

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


def floor_at(app: RpcSubprocess, size: tuple[int, int]) -> dict:
    """The floor search run with its ceiling AT `size` — which is how a caller
    asks "does this screen fit here", in the search's own predicate.

    A size the screen fits in answers a floor; one it does not answers
    `refused: ceiling_is_short` carrying what is cut there.
    """
    resp = app.request(
        "scene/size_floor", {"at": {"width": size[0], "height": size[1]}}
    )
    assert resp is not None and isinstance(resp.result, dict), "scene/size_floor answers"
    return resp.result


def reach_at(app: RpcSubprocess, size: tuple[int, int]) -> dict:
    resp = app.request("scene/scroll_reach", {"at": {"width": size[0], "height": size[1]}})
    assert resp is not None and isinstance(resp.result, dict), "scene/scroll_reach answers"
    return resp.result


def cut_names(rows: list[dict]) -> list[str]:
    return sorted({row["tag"] or row["path"] for row in rows})


# ── A: the measured floor, and the declaration it is checked against ────────


def the_floor_is_measured_and_the_declaration_agrees(
    app: RpcSubprocess, name: str
) -> dict:
    report = floor_of(app)
    needed = report.get("needed")
    ok(f"A/{name}: the floor was measured rather than refused", needed is not None)
    declared = report["declared"]["floor"]
    ok(f"A/{name}: the binding declares a floor at all", declared is not None)
    # ★★★★★ R1712 — this asserted `needed == declared` and `verdict == exact`,
    # and that was right while a screen had ONE minimum. It has two now: the
    # size its layout stops reflowing at, and the size its window stops
    # shrinking at. What the measurement is about is the first, so that is what
    # it is checked against; the window floor is checked separately, against
    # what the concession says it costs.
    concession = report.get("concession")
    ok(f"A/{name}: the binding declares a shrink policy", concession is not None)
    comfortable = concession["comfortable"]
    assert_eq(
        (needed["width"], needed["height"]),
        (comfortable["width"], comfortable["height"]),
        f"A/{name}: what the screen needs WHOLE is what its policy calls comfortable",
    )
    assert_eq(
        (declared["width"], declared["height"]),
        (concession["floor"]["width"], concession["floor"]["height"]),
        f"A/{name}: and the floor the window system was told is the policy's",
    )
    assert_eq(
        report["verdict"],
        "conceded" if concession["band"] != {"width": 0, "height": 0} else "exact",
        f"A/{name}: the verdict says which of the two cases this screen is in",
    )
    ok(
        f"A/{name}: the floor is not degenerate "
        f"({needed['width']}x{needed['height']} of a "
        f"{report['ceiling']['width']}x{report['ceiling']['height']} ceiling)",
        needed["width"] > 100 and needed["height"] > 100,
    )
    ok(
        f"A/{name}: the search says what it cost ({report['probes']} probes)",
        10 < report["probes"] < 60,
    )
    # ★★★★★ The counterfactual that found this assertion missing: with the
    # ceiling taken from the DECLARED floor instead of the live window, every
    # section above still passed — the instrument would have been measuring the
    # screen against the very number it exists to check, and it only shows on a
    # screen whose declaration is wrong, which is the case the read is for.
    ceiling = (report["ceiling"]["width"], report["ceiling"]["height"])
    assert_eq(
        ceiling,
        design_size(app),
        f"A/{name}: the search was run against the window this screen HAS, not "
        f"against the floor it claims",
    )
    ok(
        f"A/{name}: and that ceiling leaves the search somewhere to go "
        f"({ceiling} down to {needed['width']}x{needed['height']})",
        ceiling[0] >= needed["width"] and ceiling[1] >= needed["height"],
    )
    print(
        f"[demo] A/{name}: needs {needed['width']}x{needed['height']}, "
        f"declares {declared['width']}x{declared['height']}, "
        f"{report['probes']} probes"
    )
    return report


# ── B: the evidence, and the boundary in both directions ────────────────────


def the_number_carries_its_evidence(app: RpcSubprocess, name: str, report: dict) -> None:
    before = design_size(app)
    for axis in ("width", "height"):
        measured = report[axis]
        forced = measured["forced_by"]
        ok(
            f"B/{name}/{axis}: the extent names what one pixel less loses "
            f"({len(forced)}: {cut_names(forced)[:3]})",
            len(forced) > 0,
        )
        assert_eq(
            measured["short_extent"],
            measured["extent"] - 1,
            f"B/{name}/{axis}: the two ends of the boundary are one fact",
        )
        for row in forced:
            ok(
                f"B/{name}/{axis}: {row['tag'] or row['path']} says how far past "
                f"reach it goes ({row['short_by']})",
                any(edge > 0 for edge in row["short_by"]),
            )
        # ★ The boundary, driven through the hypothetical read in BOTH
        # directions — the check that makes the number falsifiable.
        other = report["height" if axis == "width" else "width"]["extent"]
        at_extent = (
            (measured["extent"], report["ceiling"]["height"])
            if axis == "width"
            else (report["ceiling"]["width"], measured["extent"])
        )
        at_short = (
            (measured["short_extent"], report["ceiling"]["height"])
            if axis == "width"
            else (report["ceiling"]["width"], measured["short_extent"])
        )
        assert other > 0
        fits = floor_at(app, at_extent)
        assert_eq(
            fits.get("refused"),
            None,
            f"B/{name}/{axis}: the extent it named is a size this screen fits in",
        )
        # ★★★★★ R1711.1 — the negative direction, and it used to be vacuous.
        # This asked `scene/scroll_reach` at the short extent and asserted
        # `marks > 0`, which is "the screen painted something" — measured, the
        # weak read answers `lost: 0` at every one of these sizes, because a
        # mark with one pixel on screen is not out of sight. So the boundary's
        # own predicate is what asks: driving the search with its ceiling AT the
        # short extent refuses, and names the same marks the axis named.
        short = floor_at(app, at_short)
        refused = short.get("refused")
        ok(
            f"B/{name}/{axis}: one pixel less is a size this screen does not fit in",
            refused is not None and refused["reason"] == "ceiling_is_short",
        )
        assert_eq(
            refused["axis"],
            None,
            f"B/{name}/{axis}: and a size that does not fit names no axis",
        )
        assert_eq(
            cut_names(refused["out_of_reach"]),
            cut_names(forced),
            f"B/{name}/{axis}: the marks it names there are the evidence the "
            f"answer carried",
        )
    assert_eq(
        design_size(app),
        before,
        f"B/{name}: none of that moved the window (§2 #3)",
    )


# ── C: the pair is a third question ─────────────────────────────────────────


def the_pair_is_checked_and_not_assumed(name: str, report: dict) -> None:
    pair = report.get("pair")
    ok(f"C/{name}: the answer says whether its own pair is a size", pair is not None)
    assert_eq(
        pair["verdict"],
        "fits",
        f"C/{name}: the two axis answers together are a size that works "
        f"(cut: {cut_names(pair['out_of_reach'])})",
    )
    assert_eq(
        pair["out_of_reach"],
        [],
        f"C/{name}: and a `fits` verdict carries no losses beside it",
    )


# ── D: the floor is real in the window ──────────────────────────────────────


def the_window_can_actually_be_that_small(
    app: RpcSubprocess, name: str, report: dict
) -> tuple[int, int]:
    needed = report["needed"]
    size = (needed["width"], needed["height"])
    resize_and_settle(app, size)
    assert_eq(design_size(app), size, f"D/{name}: the window took its measured floor")
    made = assert_declared_panes_on_screen(app, size, label=f"D/{name}")
    if made:
        CHECKS.extend(made)
    else:
        print(f"[demo] D/{name}: the specification is not organised in panes")
    declared = declared_and_painted(app, size)
    ok(
        f"D/{name}: the specification is on screen at the floor ({len(declared)} regions)",
        len(declared) >= 8,
    )
    live = app.request("scene/scroll_reach")
    assert live is not None and isinstance(live.result, dict)
    assert_eq(
        live.result["lost"],
        0,
        f"D/{name}: nothing is out of reach in the window at its floor",
    )
    return size


# ── E: the dry answer and the live one are one fact ─────────────────────────


def a_dry_ask_answers_what_the_live_read_answers(
    app: RpcSubprocess, name: str, size: tuple[int, int]
) -> None:
    live = app.request("scene/scroll_reach")
    assert live is not None and isinstance(live.result, dict)
    dry = reach_at(app, size)
    assert_eq(
        (dry["window"]["w"], dry["window"]["h"]),
        size,
        f"E/{name}: the dry answer says which window it is about",
    )
    for key in ("marks", "scrollable", "lost"):
        assert_eq(
            dry[key],
            live.result[key],
            f"E/{name}: asked about the size it already is, the dry read agrees "
            f"with the live one on `{key}`",
        )
    elsewhere = (size[0] + 400, size[1] + 300)
    away = reach_at(app, elsewhere)
    assert_eq(
        (away["window"]["w"], away["window"]["h"]),
        elsewhere,
        f"E/{name}: and a different size is answered as that size",
    )
    assert_eq(
        design_size(app),
        size,
        f"E/{name}: asking about another size did not resize the window",
    )


# ── F: R1710's five names, driven ───────────────────────────────────────────


def the_five_are_one_scroll_away_and_the_scroll_works(
    app: RpcSubprocess, name: str, size: tuple[int, int]
) -> None:
    live = app.request("scene/scroll_reach")
    assert live is not None and isinstance(live.result, dict)
    rows = {row["tag"]: row for row in live.result["out_of_sight"] if row["tag"]}
    for tag in R1710_FIVE:
        row = rows.get(tag)
        ok(f"F/{name}: {tag} is reported at the floor", row is not None)
        assert_eq(
            row["reach"],
            "scrollable",
            f"F/{name}: {tag} is one scroll away, not lost — R1710 read it as lost",
        )
        pane = row["viewport"]["name"]
        app.scroll(pane, to=(row["to_x"], row["to_y"]))
        app.tick(0.016)
        reachable = abs_rects_of(app.snapshot(source="paint", viewport=size))
        ok(
            f"F/{name}: scrolling {pane} to {(row['to_x'], row['to_y'])} makes {tag} "
            f"reachable in the paint",
            tag in reachable,
        )
    # Put the panes back so the pixel scan photographs the screen as it opens.
    for pane in {rows[tag]["viewport"]["name"] for tag in R1710_FIVE}:
        app.scroll(pane, to=(0, 0))
    app.tick(0.016)


# ── G: a malformed ask is refused by name ───────────────────────────────────


def a_malformed_ask_is_refused_by_name(app: RpcSubprocess, name: str) -> None:
    for bad in ({"width": 0, "height": 400}, {"width": 900}, [900, 400]):
        try:
            app.request("scene/scroll_reach", {"at": bad})
        except RpcError as err:
            ok(f"G/{name}: {bad!r} is refused by name ({err})", "InvalidAt" in str(err))
        else:
            raise AssertionError(f"G/{name}: {bad!r} was answered about the live window")


# ── H: pixels ───────────────────────────────────────────────────────────────


def the_floor_is_a_screen_with_ink_on_it(
    app: RpcSubprocess, name: str, size: tuple[int, int], png: Path
) -> None:
    app.request("scene/screenshot", {"path": "", "out_path": str(png)})
    assert png.exists(), f"H/{name}: no screenshot was written"
    img = read_png_rgba8(png)
    assert_eq(
        (img.width, img.height),
        size,
        f"H/{name}: the photograph is of the window at its floor",
    )
    inked = inked_samples(img)
    ok(f"H/{name}: and there is a screen in it ({inked} inked samples)", inked > 150)
    print(f"[demo] H/{name}: {size[0]}x{size[1]}, {inked} inked samples -> {png.name}")


def inked_samples(img: Png) -> int:
    """Samples carrying ink over the whole surface — scanned, not glanced at."""
    inked = 0
    for row in range(4, img.height, 11):
        for col in range(4, img.width, 7):
            r, g, b, _ = png_pixel(img, col, row)
            if abs(r - g) > 6 or abs(g - b) > 6 or r > 60:
                inked += 1
    return inked


# ── the round ───────────────────────────────────────────────────────────────


def drive(name: str, example: str, tmp: Path) -> None:
    banner(f"{name} ({example})")
    with RpcSubprocess(example) as app:
        report = the_floor_is_measured_and_the_declaration_agrees(app, name)
        the_number_carries_its_evidence(app, name, report)
        the_pair_is_checked_and_not_assumed(name, report)
        a_malformed_ask_is_refused_by_name(app, name)
        size = the_window_can_actually_be_that_small(app, name, report)
        a_dry_ask_answers_what_the_live_read_answers(app, name, size)
        if example == "hello-node-lab":
            the_five_are_one_scroll_away_and_the_scroll_works(app, name, size)
        the_floor_is_a_screen_with_ink_on_it(app, name, size, tmp / f"{example}.png")


def main() -> None:
    with tempfile.TemporaryDirectory() as d:
        for name, example in SCREENS:
            drive(name, example, Path(d))
    print(f"\n{len(CHECKS)} assertions across {len(SCREENS)} screens")
    assert len(CHECKS) >= 40, f"only {len(CHECKS)} assertions"


if __name__ == "__main__":
    run_demo("hello-node-lab", main)
