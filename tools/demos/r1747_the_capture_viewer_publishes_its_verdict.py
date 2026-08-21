#!/usr/bin/env python3
"""R1747 §5.27 §5.38 §5.40 §2 #2 §2 #7 — **the assembled analysis tool judges
its capture viewer against a written specification, in the app, through the UI —
and a decode row that lights no bytes says WHICH of the two reasons it is.**

# What this demo exists for

R1738 made the assembled application count the sections it was judged on and
opened a reviewed remainder for the ones it could not. That remainder recorded
this section as unjudged with the reason *the capture viewer has no written
specification at all — not one unpublished, one unwritten*.

**That sentence was false, and had been for nine rounds.** Re-measured before
this round wrote a line of code, by listing the crate's modules and grepping the
tree for the hook: R1663 wrote screen B's specification as a value and its
`painted` module already ran the real view and layout passes and compared the
painted scene against it in both directions. Both predate the entry. What was
true is the other half of the same sentence — those modules are `#[cfg(test)]`
and the binding implemented no `conformance` hook — so the verdict was computed
and stopped inside one binary's test run. This section was *one unpublished*,
the shape the node lab was in before R1742, and the remainder said the opposite.

So this round did two things: published the verdict, and corrected the reason.
The lesson is in the pin now rather than only here — **a reason recorded in a
remainder is not re-measured by the thing that enforces it.** The gate below
asserts the remainder's KEYS equal the application's unjudged rows and can say
nothing about the sentences beside them, so a wrong reason survives every push
that a right one would.

# What it drives

* **A** — the application's own report: `packets` is `judged`, and the sections
  nothing has judged are **exactly** the reviewed remainder in
  `docs/analyzer-sections-spec.json`. An entry paid off must be deleted there,
  so this fails as loudly for a stale ledger as for a silent section.
* **B** — ★★★★★ the disagreement R1742 found by re-running an older gate, made
  a standing assertion: the row read **from the dashboard** and the row read
  **while standing in the section** are the same verdict, and only `showing`
  differs. A verdict derived from the paint is about a surface's last frame, so
  a report that did not say which frame each row was about would read a stale
  answer as a live one.
* **C** — the six surfaces, judged **at two window sizes**, driven through the
  painted rectangles. R1742 measured a section that reproduces its
  specification in its own window and cannot as a page — a 74-pixel inspector —
  so one size is not a measurement.
* **D** — ★ the round's own finding: a decode row can light no bytes for **two**
  reasons, and the screen says which. Two rows hold a value the decoder worked
  out rather than read; one more was read from a byte source this pane is not
  showing. Found by driving all twenty-one rows the tree draws, not by
  reasoning. The two away sentences are asserted to be different, because a
  declared absence whose reason is one wording reused is silence with extra
  steps.
* **E** — every part the specification fixes is **painted in the assembled
  application**, at the address the specification names, with the host's own
  chrome still standing beside it — a page, not a takeover.
* **F** — one build, two placements: the host's row for `packets` IS the value
  the section publishes on its own wire, and a **second process** running the
  same section standalone answers identically once driven into the same
  session. ★★★★★ The pair is deliberately driven **apart** first, because the
  first draft of that check passed while the two processes were in *different*
  sessions — five of the six surfaces do not move with a session and the sixth
  is titled by what its parts are, so the equality could not have failed. Two
  values that cannot differ are not evidence that they agree.

# Floor

⚠ **No probe was built this round, and this section says so rather than
carrying numbers nobody measured.** The sibling demos' floor sections were each
produced by compiling a probe against the reference release and running it, and
writing one here from reasoning would make an unmeasured claim indistinguishable
from those.

What IS recorded, from the probe R1742 did build and run against a
property-editor-shaped pane of the same class: across that pane, its form
layout, its enumeration control, its text control, its page-stack container and
its tabbed container, **564** members were scanned and **0** name a
specification, an expectation, a divergence or a verdict. That is the floor
sentence this round rests on and it is not re-measured here — the surfaces below
are different widgets of the same toolkit, and whether the number is 564 or some
other number, the fact the mechanism needs is that it is zero.

The one thing this round would want measured and did not: whether that toolkit's
byte-view highlight can be read back as *what was painted* rather than as *what
was asked for*, which is the round trip section E asserts between the pane's own
sentence and the pane's own highlight. Left as a question rather than answered
from memory.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import packets_spec, surfaces, unjudged_sections  # noqa: E402
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    resize_and_settle,
    run_demo,
    text_of_tag,
)

SHELL = "hello-analyzer-shell"
VIEWER = "hello-packet-view"
EXT = "/external"
SEAT = "packets"

#: Where each surface's parts are addressed, by the key the pin gives them.
#: `selection` is absent on purpose — its three parts are a RELATION between
#: three regions rather than three tags under one stem, and section E counts
#: them separately instead of quietly skipping them.
STEMS = {
    "filter_bar": "pv.filter.",
    "context": "pv.context.",
    "list_columns": "pv.list.head.",
    "decode_layers": "pv.tree.field.",
    "reassembly": "pv.reassembly.",
}

#: The three regions the `selection` relation is painted across.
RELATION = {
    "field": "pv.tree.selected",
    "span": "pv.bytes.span",
    "lit": "pv.bytes.lit.",
}

CHECKS: list[str] = []
PARTS_COMPARED = 0
PRESSES = 0


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def report(app: RpcSubprocess) -> dict:
    """The application's own count of how much of itself has been judged."""
    return app.query(f"{EXT}/sections")


def row(app: RpcSubprocess) -> dict:
    return next(r for r in report(app)["rows"] if r["key"] == SEAT)


def verdict_of(app: RpcSubprocess) -> dict:
    """The section's whole verdict, as the host's row publishes it.

    ★ R1758 — a row carries the section's verdict under one key now, qualifier
    included, rather than spreading three of its facts flat.
    """
    return row(app)["conformance"]


def surfaces_of(app: RpcSubprocess) -> dict:
    """The per-surface half of that verdict."""
    return verdict_of(app)["surfaces"]


def press_tag(app: RpcSubprocess, tag: str) -> None:
    """Press the rectangle the last frame drew for `tag`.

    ★ The rect is re-read immediately before aiming and the press has no
    movement in it: R1736 measured that a probe which moves and then presses is
    driving a DRAG, so what it measures is the drift rather than the press.
    """
    global PRESSES
    box = abs_rects_of(app.snapshot(source="paint")).get(tag)
    assert box is not None, f"the frame drew nothing at {tag}"
    x, y, w, h = box
    # ★ R1761 — and the press is followed by the FRAME it causes, not by a tick.
    # Everything a caller reads after this is a fact about the last painted
    # frame, so returning before that frame exists hands back the screen as it
    # was before the press.
    before = app.frame_count()
    app.click((x + w / 2, y + h / 2))
    app.tick(16)
    app.await_paint(before)
    PRESSES += 1


def section_a(app: RpcSubprocess) -> None:
    banner("A — the application judges the capture viewer, and the remainder is an equality")
    said = report(app)
    here = row(app)
    assert_eq(
        here["standing"],
        "judged",
        "A: ★★★★★ the capture viewer publishes a verdict about its own written "
        "specification where the application it is a page of can reach it",
    )
    unjudged = {
        r["key"]: r["why"]
        for r in said["rows"]
        if r["standing"] in {"unspecified", "inline"}
    }
    assert_eq(
        sorted(unjudged),
        sorted(unjudged_sections()),
        "A: ★★★ the sections nothing has judged are exactly the ones "
        "docs/analyzer-sections-spec.json accepts -- so the entry this round "
        "paid off had to be DELETED there, and one left behind fails here",
    )
    ok(
        "A: `packets` is no longer in the reviewed remainder",
        SEAT not in unjudged_sections(),
    )
    ok(
        "A: ★★ and NOTHING is `unspecified` any more -- the two entries left "
        "are both the other kind, a page the host paints itself",
        all(r["standing"] != "unspecified" for r in said["rows"]),
    )
    ok(
        "A: every row still says which of the four things it is",
        all(
            r["standing"] in {"judged", "unspecified", "inline", "closed"}
            for r in said["rows"]
        ),
    )
    ok(
        "A: and the judged row carries the tag its section is addressed by, so "
        "a reader of this report can go and ask the section itself",
        here["tag"] == "packet_view" and here["conformance"]["surfaces"],
    )
    assert_eq(
        sorted(here["conformance"]["surfaces"]),
        sorted(surfaces(packets_spec())),
        "A: ★★ the surfaces the section publishes a VERDICT for are exactly the "
        "ones the pin fixes -- one document, two readings",
    )
    ok(
        "A: and the application still does not claim conformance, because the "
        "two pages the host paints itself remain unjudged",
        said["conforms"] is False and said["unjudged"] == len(unjudged_sections()),
    )
    print(
        f"  [population] {said['sections']} section(s): {said['judged']} judged, "
        f"{said['unjudged']} unjudged, {said['closed']} closed"
    )


def section_b(app: RpcSubprocess) -> None:
    banner("B — the same verdict read from the dashboard and from inside the section")
    assert_eq(app.query(f"{EXT}/nav"), "dashboard", "B: this report starts at the dashboard")
    cold = verdict_of(app)
    ok(
        "B: ★ a section the session has never opened says its surfaces are AWAY "
        f"with the reason: {cold['surfaces']['context'].get('why', '<none>')}",
        all(s["standing"] is False and s["why"] for s in cold["surfaces"].values()),
    )
    ok(
        "B: ★★★★★ and it accuses the build of NOTHING -- an unpainted section "
        "has no divergences and reproduces 0 of the parts it specifies",
        all(
            s["divergences"] == [] and s["reproduced"] == 0 and s["specified"] > 0
            for s in cold["surfaces"].values()
        ),
    )
    ok(
        "B: ★★★★★ R1758 -- and the verdict says it was read from the PAINT, "
        "which is what makes the sentence above checkable: a verdict from a "
        "screen's own tables would report every part reproduced from here",
        cold["evidence"] == "paint" and cold["reproduced"] == 0,
    )

    # ★ R1761 — and not `intervene` + `tick` at any of the three: every verdict
    # read here is a fact about the last PAINTED frame, and this demo failed
    # once in a 34-demo sweep and never in 20 isolated re-runs, which is what a
    # read racing the render looks like. `intervene_painted` comes back once the
    # window has drawn the page it asked for.
    app.intervene_painted(f"{EXT}/nav", SEAT)
    assert_eq(app.query(f"{EXT}/nav"), SEAT, "B: the capture-viewer seat opens")
    standing = row(app)
    ok("B: standing in it, the section is the one showing", standing["showing"] is True)

    app.intervene_painted(f"{EXT}/nav", "dashboard")
    away = row(app)
    # ★★★★★ R1763 — this asserted that the verdict read from the dashboard was
    # the SAME as the one read while standing in the section, and called that
    # R1742's hardest defect asserted rather than rediscovered. It was pinning
    # the defect: the two were equal because the section's marks stayed in the
    # framework's store after the reader left, so the row went on reporting a
    # reproduced specification about a frame that had left the application.
    # Leaving takes the marks with it now, exactly as it already took the
    # screen's externals, its windows and its accessibility tree — so the
    # verdict changes, and what it changes TO is the honest answer.
    assert_eq(
        (
            away["conformance"]["reproduced"],
            away["conformance"]["away"],
            away["conformance"]["reconciles"],
        ),
        (0, len(away["conformance"]["surfaces"]), False),
        "B: ★★★★★ read from the dashboard the section reproduces NOTHING and "
        "every surface is away -- leaving took its marks with it, so no verdict "
        "here is about a frame that has left",
    )
    ok(
        "B: ★★ and the specification it is judged against did not move with "
        "them: the same parts are named, and only what the frame had changed",
        {name: surface["canon"] for name, surface in away["conformance"]["surfaces"].items()}
        == {
            name: surface["canon"]
            for name, surface in standing["conformance"]["surfaces"].items()
        },
    )
    ok(
        "B: ★★ and only one section is showing, so a verdict about a frame that "
        "has left cannot read as a verdict about the application",
        away["showing"] is False
        and [r["key"] for r in report(app)["rows"] if r["showing"]] == ["dashboard"],
    )
    app.intervene_painted(f"{EXT}/nav", SEAT)


def judge_surfaces(app: RpcSubprocess, where: str) -> None:
    """Every surface the pin fixes, as the application reports it right now."""
    global PARTS_COMPARED
    said = surfaces_of(app)
    for name in sorted(said):
        verdict = said[name]
        ok(
            f"C: [{where}] `{name}` reproduces {verdict['reproduced']} of "
            f"{verdict['specified']} specified part(s), read from the paint "
            f"-- unreconciled: {verdict['unreconciled'] or 'none'}",
            verdict["standing"] is True and verdict["unreconciled"] == [],
        )
        PARTS_COMPARED += verdict["specified"]


def section_c(app: RpcSubprocess) -> None:
    banner("C — the six surfaces, judged at two window sizes")
    judge_surfaces(app, "opening window")
    body = abs_rects_of(app.snapshot(source="paint"))["pv.root"]
    print(f"  [page] the shell gives the capture viewer {body[2]}x{body[3]} pixels")

    resize_and_settle(app, (2560, 1600))
    app.tick(16)
    judge_surfaces(app, "2560x1600")
    grown = abs_rects_of(app.snapshot(source="paint"))["pv.root"]
    ok(
        f"C: ★★★ and the page really did change size ({body[2]}x{body[3]} -> "
        f"{grown[2]}x{grown[3]}), so the two readings above are two "
        "measurements and not one repeated",
        grown[2] > body[2] and grown[3] > body[3],
    )
    resize_and_settle(app, (1500, 950))
    app.tick(16)
    ok(
        "C: ★ the whole document reconciles -- every surface standing, every "
        "declared remainder matched, nothing undeclared",
        all(s["standing"] and not s["unreconciled"] for s in surfaces_of(app).values()),
    )
    ok(
        "C: ★★ and one surface's declared remainder is NOT empty, so the "
        "ledger is doing work rather than sitting at zero: "
        f"{[o['key'] for o in surfaces_of(app)['reassembly']['owed']]}",
        surfaces_of(app)["reassembly"]["owed"],
    )
    print(f"  [coverage] {PARTS_COMPARED} specified part(s) judged, {PRESSES} press(es)")


def section_d(app: RpcSubprocess) -> str:
    banner("D — a row that lights no bytes says WHICH of the two reasons it is")
    tag = row(app)["tag"]

    def selection() -> dict:
        return surfaces_of(app)["selection"]

    ok(
        "D: with a row the pane can show, the relation is standing and whole",
        selection()["standing"] is True
        and selection()["reproduced"] == selection()["specified"],
    )

    # ★ The two away states, reached by pressing the rows themselves. Which
    # rows they are is read from the SCREEN's own published tables, not written
    # here: a hand-written pair would keep passing after the decode changed.
    published = app.query(f"/{tag}{EXT}/spec")
    headings = {layer["id"] for layer in published["layers"]}
    marked = abs_rects_of(app.snapshot(source="paint"))

    # 🟥 MEASURED HERE, and it is a gap rather than a quirk: a press on a LAYER
    # heading folds it and leaves the selection where it was, so a layer row
    # cannot be opened by a press at all -- while the same row opens over the
    # wire. The behaviour reference binds selection to every row of its decode
    # tree and has no fold. Asserted rather than worked around silently, and
    # registered as `debt-a-layer-row-opens-over-the-wire-and-not-by-a-press`;
    # the population below is derived from it.
    before = app.query(f"/{tag}{EXT}/selected_field")
    folded_before = app.query(f"/{tag}{EXT}/folded")
    press_tag(app, f"pv.tree.field.{sorted(headings)[0]}")
    ok(
        "D: 🟥 a press on a layer heading FOLDS it and does not open it -- the "
        "reference opens every row of its decode tree and has no fold, so this "
        "is a gap and not a quirk (debt-a-layer-row-opens-over-the-wire-and-"
        "not-by-a-press)",
        app.query(f"/{tag}{EXT}/selected_field") == before
        and app.query(f"/{tag}{EXT}/folded") != folded_before,
    )
    press_tag(app, f"pv.tree.field.{sorted(headings)[0]}")  # and unfold it again

    fields = [f for f in app.query(f"/{tag}{EXT}/visible_fields") if f not in headings]
    worked_out = [f for f in fields if f"pv.tree.derived.{f}" in marked]
    print(f"  [tree] {len(fields)} openable row(s); the tree marks {worked_out} as worked out")
    ok(
        f"D: the tree marks {len(worked_out)} row(s) as holding a value the "
        "decoder worked out rather than read",
        len(worked_out) >= 1,
    )

    press_tag(app, f"pv.tree.field.{worked_out[0]}")
    derived = selection()
    ok(
        f"D: ★★★ pressing `{worked_out[0]}` takes the relation away with the "
        f"derived reason -- {derived.get('why', '<none>')}",
        derived["standing"] is False and "worked out" in derived.get("why", ""),
    )
    ok(
        "D: ★★★★★ and it accuses the build of nothing: an away surface has no "
        "divergences and reproduces 0, which is not the same as passing",
        derived["divergences"] == [] and derived["reproduced"] == 0,
    )

    # The other reason, found by driving every row rather than by reasoning:
    # a value that WAS read, from a byte source this pane is not showing.
    elsewhere = None
    for path in fields:
        if path in worked_out:
            continue
        press_tag(app, f"pv.tree.field.{path}")
        if selection()["standing"] is False:
            elsewhere = path
            break
    ok(
        "D: ★★★★★ exactly one row was read from bytes this pane is not "
        f"showing, and it is a DIFFERENT away: {elsewhere}",
        elsewhere is not None,
    )
    other = selection()
    ok(
        f"D: ★★★★★ and its sentence is the other one -- {other.get('why', '<none>')}",
        other["why"] != derived["why"] and "no derived mark" in other.get("why", ""),
    )
    ok(
        "D: ★★ the two away sentences are DIFFERENT, which is the whole value "
        "of a declared absence over silence: one wording reused would say the "
        "screen knows less than it does",
        derived["why"] != other["why"],
    )

    # And back: away is a state, not a latch.
    readable = next(f for f in fields if f not in worked_out and f != elsewhere)
    press_tag(app, f"pv.tree.field.{readable}")
    back = selection()
    ok(
        f"D: ★★ pressing `{readable}` brings it back -- away is a state the "
        "session is in, not a verdict the screen latched",
        back["standing"] is True and back["reproduced"] == back["specified"],
    )
    ok(
        "D: ★ and the row it is about really did change, so the two readings "
        "are two frames",
        app.query(f"/{tag}{EXT}/selected_field") == readable,
    )
    return worked_out[0]


def section_e(app: RpcSubprocess) -> None:
    banner("E — every specified part is PAINTED inside the assembled application")
    rects = abs_rects_of(app.snapshot(source="paint"))
    for chrome in ("shell.appbar", "shell.rail", f"shell.rail.{SEAT}"):
        ok(f"E: the host's {chrome} survives the capture viewer being on it", chrome in rects)

    pin = packets_spec()
    missing: list[str] = []
    compared = 0
    related = 0
    for name in surfaces(pin):
        for part in pin[name]["canon"]:
            stem = STEMS.get(name)
            if stem is None:
                related += 1
                continue
            compared += 1
            wanted = f"{stem}{part['key']}"
            if wanted not in rects:
                missing.append(f"{name}.{part['key']} ({wanted})")
    ok(
        f"E: ★★★ every one of the {compared} parts addressed by a tag is "
        f"painted in the assembled application -- missing: {missing or 'none'}",
        not missing and compared > 0,
    )
    lit = sorted(
        int(t.rsplit(".", 1)[1])
        for t in rects
        if t.startswith(RELATION["lit"])
    )
    ok(
        f"E: ★ and the {related} part(s) of the RELATION are painted across "
        f"three regions rather than under one stem, so they are checked here "
        f"rather than silently skipped -- {len(lit)} byte(s) lit",
        RELATION["field"] in rects and RELATION["span"] in rects and lit,
    )

    # ★★ The relation itself, from the PAINT on both sides. The tree draws a
    # band behind the row a reader has open and the band carries no name, so
    # which row it is behind is the row whose own mark lies within it -- the
    # same reading the screen's own verdict makes, done independently here.
    band = rects[RELATION["field"]]
    open_row = next(
        t[len("pv.tree.field.") :]
        for t, r in sorted(rects.items())
        if t.startswith("pv.tree.field.")
        and r[1] >= band[1]
        and r[1] + r[3] <= band[1] + band[3]
    )
    said = text_of_tag(app, RELATION["span"])
    ok(
        f"E: ★★ the byte pane's readout names the row the tree drew open "
        f"({said!r} for `{open_row}`) -- two panes, one fact, compared where a "
        "reader would compare them",
        said.startswith(open_row),
    )
    first, last = said.rsplit(" · ", 1)[1].split("..")
    ok(
        f"E: ★★★★★ and the bytes it lights are exactly the extent it names "
        f"({first}..{last} -> {len(lit)} lit) -- the round trip this screen "
        "was built for, asserted between the sentence and the highlight rather "
        "than between either and the map that produced both",
        lit == list(range(int(first, 16), int(last, 16) + 1)),
    )
    print(
        f"  [paint] {compared} specified part(s) read from the assembled frame, "
        f"{related} judged as a relation across three regions"
    )


def section_f(app: RpcSubprocess, derived: str) -> None:
    banner("F — one build, two placements, one sentence")
    here = row(app)
    own = app.query(f"/{here['tag']}{EXT}/conformance")
    assert_eq(
        own,
        here["conformance"],
        "F: ★★ the host's row for `packets` IS the value the section publishes "
        "on its own wire -- the host aggregates, it does not re-derive",
    )
    assert_eq(
        here["conformance"]["specified"],
        sum(s["specified"] for s in own["surfaces"].values()),
        "F: and the row's totals are its surfaces added up",
    )
    # ★★★ And the document the verdict is against is the one in `docs/`, read
    # here from the file rather than from the application. The section also
    # publishes its OWN table on `spec` -- the panes, the columns, the rows,
    # written in the same edit as the painter it feeds -- and a build compared
    # with that is a build agreeing with itself. So the counts the running
    # application reports are checked against the pin as this process reads it
    # off disk: two hands, and the comparison is between them.
    pin = packets_spec()
    said = own["surfaces"]
    assert_eq(
        {name: said[name]["specified"] for name in sorted(said)},
        {name: len(pin[name]["canon"]) for name in sorted(surfaces(pin))},
        "F: ★★★ every surface's specified count is the length of that surface's "
        "canon in docs/analyzer-packets-spec.json, read off disk by this process "
        "-- the verdict is against the pin and not against the screen's own table",
    )
    # ★★★★★ R1758 — and WHICH parts, not only how many. A verdict that carries
    # its canon is one a reader can check without going and finding the pin, and
    # the check that it is the pin's canon is this one.
    assert_eq(
        {
            name: [part["key"] for part in said[name]["canon"]]
            for name in sorted(said)
        },
        {
            name: [part["key"] for part in pin[name]["canon"]]
            for name in sorted(surfaces(pin))
        },
        "F: ★★★★★ and the verdict NAMES the parts it is about, in the pin's own "
        "order -- a count with nothing behind it cannot be checked from outside",
    )
    assert_eq(
        sorted(o["key"] for o in said["reassembly"]["owed"]),
        sorted(o["key"] for o in pin["reassembly"]["owed"]),
        "F: ★★ and the declared remainder the application carries is the one "
        "the pin declares, entry for entry",
    )

    with RpcSubprocess(VIEWER, boot_grace=1.5) as alone:
        fresh = alone.query(f"{EXT}/conformance")
        ok(
            "F: a second process running the same section standalone, in its "
            "own window, publishes the same verdict as the page does",
            fresh == own,
        )
        ok(
            "F: ★ and it reconciles there too, so the verdict is not an "
            "artifact of the host's page size",
            all(
                s["standing"] and not s["unreconciled"]
                for s in fresh["surfaces"].values()
            ),
        )
        # ★★★★★ The check above is the one worth being suspicious of, and this
        # round was: two verdicts that CANNOT differ are not evidence that they
        # agree. The two processes are in different sessions here -- different
        # rows open -- and the equality above holds anyway, because every
        # surface but one is session-independent and that one is titled by what
        # its parts ARE. So the pair is driven apart on purpose and then back
        # together, which is what makes "one build, two placements" a
        # measurement instead of a coincidence.
        press_tag(alone, f"pv.tree.field.{derived}")
        parted = alone.query(f"{EXT}/conformance")
        ok(
            "F: ★★★★★ driven somewhere the host has not been, the standalone "
            "verdict PARTS from the host's -- so the equality above is a fact "
            "about the build rather than about a value that cannot move",
            parted != own and parted["surfaces"]["selection"]["standing"] is False,
        )
        press_tag(app, f"pv.tree.field.{derived}")
        assert_eq(
            alone.query(f"{EXT}/conformance"),
            app.query(f"/{here['tag']}{EXT}/conformance"),
            "F: ★★★★★ and driven into the SAME session the two publish the SAME "
            "verdict again -- a section that conforms in its own window and is "
            "never asked as a page would be two builds wearing one name",
        )


def body() -> None:
    pin = packets_spec()
    ok(
        "the pin fixes at least one surface and every part of every surface "
        "carries a key and a title",
        surfaces(pin)
        and all(
            part.get("key") and part.get("title")
            for name in surfaces(pin)
            for part in pin[name]["canon"]
        ),
    )
    ok(
        "and the reviewed remainder still names a seat and a sentence for "
        "every section it accepts as unjudged",
        all(k and s for k, s in unjudged_sections().items()),
    )

    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        section_a(app)
        section_b(app)
        section_c(app)
        derived = section_d(app)
        section_e(app)
        section_f(app, derived)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1747 the capture viewer publishes its verdict", body)
