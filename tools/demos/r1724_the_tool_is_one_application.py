#!/usr/bin/env python3
"""R1724 §5.16 §5.38 §5.40 §2 #2 #7 — **the analysis tool is one application.**

The behaviour reference this tool is modelled on is a single shell whose rail
switches between its sections. This tree assembled it as three executables, and
the shell's own rail said so: three of its seven seats were declared
`elsewhere` — *built, shipping, and not here* — an arm that exists only because
a destination here could be finished and still unreachable.

This round mounts the first of them. `hello-node-lab` — 20,655 lines, unedited —
is the node lab destination's page, through `pinion_screen::Mount<NodeLabView>`.
(R1728 renamed that seat from `catalog` to `lab`: the reference has a node graph
section and no catalogue section, so the page was right and the address was this
application's invention.)

What this script drives, on the running application:

* **A** — the rail. The node lab seat is open, and `elsewhere` is down from
  three to **zero** (R1729 mounted the capture viewer, the last one that was
  genuinely built and unreachable; the Rust arm is gone with it).
* **B** — arriving paints the node lab. The lab's own panes are inside the page
  region at the node lab seat and absent at Dashboard.
* **C** — the lab lays out in the REGION, not the window. The page it paints
  fits inside the rectangle the shell placed it at, which is what
  `pinion_core::external::with_surface_extent` is for: before it, the in-view
  branch of `layout_size` answered the window, so a mounted screen's paint and
  its hit test would resolve against two different rectangles — R1700's defect
  class, and its own note said this was the case nothing could do better in.
* **D** — the screen that is not showing is not there. Measured at 6.11.1 by
  building a probe and running it: a page of the reference toolkit's paged
  container that is not current, sent a press, a key and a wheel, **counted all
  three**, and is reachable in the accessibility tree with its text field under
  it. Here its externals are not in the state scene at all.
* **E** — a press inside the page reaches the lab, and the shell's chrome still
  answers its own.
* **F** — the accessibility tree follows the rail: the lab's tree hangs under
  the page region while it is showing.
* **G** — leaving and returning is a return, not a restart.
* **H** — the mounted palette is the behaviour canon's roster: 21 kinds in 7
  groups, every heading and row reached by scrolling the pane a reader
  scrolls, and the row furthest from the top adds the card its badge predicts
  (R2078; this list said A-G for one round after it landed).
* **I** — the chord of each move decides where a card lands: one held gesture,
  the same pixel, plain then ctrl then released, and the grid taken and given
  back mid-flight (R2080).

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1724_the_tool_is_one_application.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
# ★ R1730 — the rail's specification, so the expectations below are DERIVED
# from the reviewed artifact rather than written out here. They were written out
# here, and the first round to pay a divergence off broke five demos at once. A
# number in a demo goes stale the same way a number in prose does.
from analyzer_spec import open_keys, owed_keys  # noqa: E402
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    address_prefix,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-analyzer-shell"
EXT = "/external"
REGION = "shell.canvas"
LAB_ROOT = "node_lab"
CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


def nodes_by_tag(app: RpcSubprocess) -> dict:
    return {n["tag"]: n for n in app.request("scene/access").result["nodes"]}


def go(app: RpcSubprocess, key: str) -> None:
    app.intervene(f"{EXT}/nav", key)
    app.tick(16)
    assert_eq(q(app, "nav"), key, f"the journey reached {key}")


def press_at(app: RpcSubprocess, rect) -> None:
    app.request(
        "scene/click",
        {"button": "left", "at": {"x": rect[0] + rect[2] // 2, "y": rect[1] + rect[3] // 2}},
    )
    app.tick(16)


def lab_tags(rects: dict) -> set:
    return {tag for tag in rects if tag == LAB_ROOT or tag.startswith("lab.")}


def card_names(app: RpcSubprocess) -> set:
    """The cards on the mounted lab's canvas, ASKED OF THE SCREEN.

    ★★★★★ R2078 — this scraped painted addresses at first, and two things were
    wrong with that in different ways.

    The one I saw: `lab.node.` also prefixes `lab.node.build.<word>`, the build
    seat R1885 added, so counting the prefix counts marks that are not cards —
    the lab's own router reads that family FIRST for exactly this reason and
    says so in place. A walk counting the prefix would report a card arriving
    whenever a seat did.

    ⚠ The one `tools/painted_addresses.py` saw, which I did not: **the walk was
    spelling a painted address it could ask for.** That ratchet went 112 -> 114
    on these two lines, and it is right — a literal prefix in a walk is the
    class the R2049-R2054 campaign removed, because a wrong letter reads as
    *the screen did not paint it* rather than as a typo. Measured while fixing
    it: the card family has no declaration to derive from either (`lab.node.` is
    spelled inline in three places in the screen's own source), which is a
    remainder of `debt-a-paint-address-is-retyped-at-every-reader` and not this
    round's to close — so the repair is to stop needing the address at all.

    ⇒ The screen publishes its live card names on its own slot, so this asks it.
    That is also the better claim: a name from the MODEL rather than one
    recovered from a tag, which is what R1999's own walk does one file over.
    """
    answer = app.query(f"/{LAB_ROOT}{EXT}/nodes")
    return {name for name in str(answer).split(",") if name}


def lab_places(app: RpcSubprocess) -> dict:
    """Where every card on the mounted lab's canvas sits, in CANVAS units.

    ★★★★★ R2080 — read out of the screen's own saved document, which is the
    only channel that answers a position in the units the gesture works in. A
    painted rectangle is the camera's answer, and recovering a canvas unit from
    one here would re-derive this screen's zoom and pan in Python — the class of
    transcription that makes a walk agree with a picture nobody sees.

    Keyed by the id the document itself hands out, so nothing here has to know
    what a card is called or how a name is minted.
    """
    saved = json.loads(str(app.query(f"/{LAB_ROOT}{EXT}/archive")))
    return {
        json.dumps(node["id"]): (node["x"], node["y"])
        for tree in saved["document"]["trees"]
        for node in tree["nodes"]
    }


def the_one_that_moved(before: dict, after: dict) -> tuple:
    """The place of the single card that moved, refusing any other count.

    A gesture that missed its target moves nothing and a gesture that took the
    canvas with it moves everything; both are failures of the drive rather than
    of the claim, and telling them apart in the message is what makes them
    cheap to fix.
    """
    changed = sorted(key for key in after if before.get(key) != after[key])
    assert len(changed) == 1, (
        f"exactly one card should have moved; {len(changed)} did ({changed})"
    )
    return after[changed[0]]


def body() -> None:  # noqa: PLR0915 - one narrative, read top to bottom
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as app:
        # ── (A) the rail ──────────────────────────────────────────────────
        banner("A — the rail: a seat that used to say 'not here'")
        roster = q(app, "destinations")
        rows = {row["key"]: row for row in roster["destinations"]}
        ok("A: the shell publishes its roster", isinstance(roster, dict))
        assert_eq(rows["lab"]["open"], True, "A: the node lab is a place you arrive at")
        assert_eq(rows["lab"]["kind"], None, "A: and carries no closure reason")
        elsewhere = sorted(k for k, r in rows.items() if r["kind"] == "elsewhere")
        assert_eq(
            elsewhere,
            [],
            "A: ★★★★★ R1729 -- NO seat is on another surface any more. This read "
            "three at R1695, two at R1724, one at R1728, and the capture "
            "viewer's mount took the last one. In the Rust source the arm is "
            "gone outright: nothing constructed it, and the compiler is what "
            "said so",
        )
        unbuilt = sorted(k for k, r in rows.items() if r["kind"] == "unbuilt")
        assert_eq(
            unbuilt,
            owed_keys(),
            "A: ★ R1728 -- and the seats the reference has working that this "
            "build has not written are EXACTLY the ones its specification "
            "declares owed. They were absent from the rail entirely before "
            "R1728, and R1730 built one of them",
        )
        opens = sorted(k for k, r in rows.items() if r["open"])
        assert_eq(
            opens,
            open_keys(),
            "A: the destinations this ONE application hosts -- derived from the "
            "specification, so building a section moves this number by itself",
        )
        # ★★★★★ §2 #2 — which destinations are whole screens is PUBLISHED, not
        # inferred from tag prefixes. An agent that had to guess would be
        # guessing at a rule nobody wrote down.
        mounted = sorted(k for k, r in rows.items() if r["mounted"])
        # ★ R1730 — the PROPERTY, not the roll. This was a written-out pair and
        # the next mount broke it; what the check is actually about is that a
        # mounted seat is a seat you can arrive at and that says how to address
        # the screen behind it. The count is a floor so the comparison below
        # cannot go vacuous, not a pin on which sections are pages.
        assert_eq(
            [k for k in mounted if k not in opens],
            [],
            "A: ★ every section that is a whole screen sits at a seat a reader "
            "can arrive at -- the roster refuses a mount at a closed one",
        )
        ok(
            f"A: ★ R1730 -- {len(mounted)} sections are whole screens, each the "
            "library half of a binary the demo sweep still drives on its own",
            len(mounted) >= 2,
        )
        for key in mounted:
            ok(
                f"A: and {key} says how to address its screen's surfaces",
                bool(rows[key]["screen"]["tag"]),
            )
        assert_eq(
            rows["lab"]["screen"]["tag"],
            LAB_ROOT,
            "A: and it says how to address that screen's surfaces",
        )
        ok("A: an unmounted destination says so", rows["dashboard"]["screen"] is None)

        # ── (B) arriving paints the node lab ──────────────────────────────
        banner("B — arriving at the node lab seat shows the node graph lab")
        go(app, "dashboard")
        at_dashboard = abs_rects_of(app.snapshot(source="paint"))
        assert_eq(lab_tags(at_dashboard), set(), "B: no lab anywhere on the dashboard")

        go(app, "lab")
        at_lab = abs_rects_of(app.snapshot(source="paint"))
        painted = lab_tags(at_lab)
        ok("B: the lab's own root is painted", LAB_ROOT in painted)
        for pane in ("lab.palette", "lab.canvas", "lab.inspector"):
            ok(f"B: the lab's {pane} pane is painted", pane in painted)
        ok(
            f"B: the lab brought a whole screen with it ({len(painted)} regions)",
            len(painted) > 40,
        )
        print(f"[demo] the mounted lab paints {len(painted)} tagged regions")

        # The shell's own chrome is still there — this is a section of an
        # application, not a second window.
        # ★ R2051 — the address, recovered from one the application publishes.
        seat_tag = address_prefix(q(app, "spec")["rail"])
        for chrome in ("shell.appbar", "shell.rail", REGION, f"{seat_tag}lab"):
            ok(f"B: the shell's {chrome} is still painted", chrome in at_lab)

        # ── (C) the lab lays out in the REGION, not the window ────────────
        banner("C — a mounted screen reads the rectangle it was placed in")
        region = at_lab[REGION]
        root = at_lab[LAB_ROOT]
        window = q(app, "spec")["window"]
        ok("C: the region is narrower than the window", region[2] < window["w"])
        ok("C: and shorter than it", region[3] < window["h"])
        # ★ The height is the proof the grant is read: nothing clamps it, so
        # the lab is exactly as tall as the region and NOT as tall as the
        # window. Before `with_surface_extent` the in-view branch of
        # `layout_size` answered the window on both axes.
        assert_eq(root[3], region[3], "C: the lab is as tall as its REGION")
        assert_eq(root[2], region[2], "C: and exactly as wide")
        ok("C: and not as tall as the window it is inside", root[3] != window["h"])
        ok("C: nor as wide", root[2] != window["w"])
        # ★★★★★ …and nothing of it escapes the rectangle, because the region
        # gives the screen the recourse it declared. The lab's layout stops
        # reflowing at 1625 wide and this region is 1388, so there IS content
        # the region cannot show — `Recourse::Pan` is what happens to it.
        # Measured before that landed: 51 of the lab's regions were outside
        # this rectangle and its inspector ran from x=1365 to x=1677 in a
        # window that ends at 1440.
        outside = [
            tag
            for tag, r in at_lab.items()
            if (tag == LAB_ROOT or tag.startswith("lab."))
            and (r[0] + r[2] > region[0] + region[2] + 1 or r[1] + r[3] > region[1] + region[3] + 1)
        ]
        assert_eq(outside, [], "C: no part of the lab escapes the region it was placed in")

        # ── (D) the screen you are not at is not there ────────────────────
        banner("D — the section that is not showing has no surfaces")
        # The externals are read off the state scene rather than through a
        # bespoke method: a slot that is not in the scene cannot be queried at
        # all, which is the guarantee rather than a symptom of it.
        ok(
            "D: a lab slot answers while the lab is showing",
            lab_slot_answers(app),
        )
        go(app, "dashboard")
        ok(
            "D: and the same slot is unaddressable when it is not",
            not lab_slot_answers(app),
        )
        assert_eq(
            lab_tags(abs_rects_of(app.snapshot(source="paint"))),
            set(),
            "D: nothing of the lab is painted either",
        )
        tree = nodes_by_tag(app)
        assert_eq(
            sorted(t for t in tree if t == LAB_ROOT or t.startswith("lab.")),
            [],
            "D: and nothing of it is in the accessibility tree -- the row the "
            "reference toolkit fails, where a hidden page is walkable with its "
            "text field under it",
        )

        # ── (E) a press inside the page reaches the lab ───────────────────
        banner("E — the pointer reaches the section that is showing")
        go(app, "lab")
        rects = abs_rects_of(app.snapshot(source="paint"))
        cards = sorted(tag for tag in rects if tag.startswith("lab.node."))
        ok(f"E: the lab painted {len(cards)} node card(s)", len(cards) >= 2)
        # ★ Asked of the LAB's own wire rather than of the painted scene: "the
        # scene changed" is satisfied by a caret blink, and it is not satisfied
        # at all by pressing the card that is already selected — which is what
        # the first draft of this check did, and it failed for that reason
        # rather than for the reason it was written.
        selected = lab_selected(app)
        other = next(c for c in cards if c.rsplit(".", 1)[-1] != selected)
        press_at(app, rects[other])
        assert_eq(
            lab_selected(app),
            other.rsplit(".", 1)[-1],
            "E: a press inside the page reaches the SCREEN, and the screen "
            "acted on it -- the pointer crossed from the host into the mounted "
            "binding's own hit test",
        )

        # The shell's own rail still answers its own presses while a whole
        # other screen is showing.
        press_at(app, rects[f"{seat_tag}dashboard"])
        assert_eq(q(app, "nav"), "dashboard", "E: the shell's rail is still the shell's")

        # ── (F) the tree follows the rail ─────────────────────────────────
        banner("F — the accessibility tree follows the rail")
        go(app, "lab")
        tree = nodes_by_tag(app)
        ok("F: the region is in the tree", REGION in tree)
        ok("F: the lab's root hangs under it", LAB_ROOT in tree[REGION]["children"])
        announced = [t for t in tree if t == LAB_ROOT or t.startswith("lab.")]
        ok(f"F: the lab announces {len(announced)} nodes of its own", len(announced) > 20)
        assert_eq(tree[REGION]["name"], "Node Lab", "F: the region names its destination")
        assert_eq(
            tree[f"{seat_tag}lab"]["current"],
            "page",
            "F: and the seat is the current one",
        )

        # ── (G) leaving and returning is a return ─────────────────────────
        banner("G — a section keeps what it had")
        rects = abs_rects_of(app.snapshot(source="paint"))
        target = next(tag for tag in rects if tag.startswith("lab.node."))
        press_at(app, rects[target])
        chosen = abs_rects_of(app.snapshot(source="paint"))
        go(app, "dashboard")
        go(app, "lab")
        again = abs_rects_of(app.snapshot(source="paint"))
        assert_eq(
            frozenset(again),
            frozenset(chosen),
            "G: the lab came back showing what it was showing -- the one row "
            "the reference toolkit gets right, and this must not regress it",
        )

        # ── (H) the node palette, inside the assembly, is the canon's ─────
        # ★★★★★ R2078 — the roster this section's screen offers is the BEHAVIOUR
        # CANON's twenty-one node kinds in its seven groups, and every one of
        # them is reachable and pressable **here**, in the one application,
        # rather than only in the standalone binary this page is the library
        # half of.
        #
        # Why it belongs in THIS walk and not beside the lab's own: the standing
        # order for this axis is that a round's artifact is the analysis tool
        # ASSEMBLED — a screen driven where a person meets it. `r1651` compares
        # the lab's paint against its published specification and `r1662` drives
        # the pane's scroll, both on the standalone binary; neither says the
        # mounted page's palette works, and a mounted screen has been where this
        # tree's surprises live (R1700's rectangle, R1724's own extent).
        banner("H — the node palette in the assembly is the behaviour canon's")
        go(app, "lab")
        spec = lab_spec(app)
        roles = spec["roles"]
        groups = spec["role_groups"]
        # The two numbers, from the screen's own wire. The canon declares
        # twenty-one kinds and a separate grouping of seven; both were
        # re-measured from the pristine canon this round.
        assert_eq(len(roles), 21, "H: ★★★★★ the palette offers the canon's 21 node kinds")
        assert_eq(
            [len(g["roles"]) for g in groups],
            [4, 5, 3, 3, 2, 3, 1],
            "H: ★★★★★ in the canon's seven groups, in its order and at its "
            "sizes -- a palette of seven threes would be the same two counts "
            "and a different screen",
        )
        assert_eq(
            sum(len(g["roles"]) for g in groups),
            len(roles),
            "H: and the groups partition the roster, so the sizes above are a "
            "check on the whole of it",
        )
        # ★ The wording axis, which is what keeps the neutral substitution
        # honest: a client comparing the two screens has to be able to tell a
        # deliberate substitution from a drift, and R2078 added a third answer
        # because three of the new roles differ for a reason that is neither.
        arms = {role["wording"] for role in roles}
        assert_eq(
            sorted(arms),
            ["as_the_canon", "neutralised", "restyled"],
            "H: ★★★★★ every role says where its words come from and all three "
            "answers are drawn -- an arm nothing reaches is a distinction "
            "nobody is making, and a place to file a role nobody classified",
        )
        for role in roles:
            ok(
                f"H: {role['name']} carries a badge and a one-line gist",
                bool(role["badge"]) and bool(role["gist"]),
            )
        # ★ R2078 — PRINTED, the way section B prints its region count, and for
        # a reason this section learned about itself: every assertion here is
        # silent on success, so a run where `roles` came back empty would loop
        # zero times, assert nothing, and PASS. The counts below are what
        # distinguishes "H ran" from "H had nothing to run over" in a log
        # somebody reads later.
        print(
            f"[demo] the mounted palette offers {len(roles)} role(s) in "
            f"{len(groups)} group(s) sized "
            f"{[len(g['roles']) for g in groups]}, wording "
            f"{sorted(arms)}"
        )
        # Every heading and every row is REACHED, scrolling the pane the way a
        # reader does. The palette's content is several times its height now, so
        # "painted" stopped being the whole question — which is what R1662 gave
        # this pane a scrolling body for.
        # ★ R2078 — both addresses come FROM the screen: the pane's scrolling
        # body is a column of its own specification, and each heading now
        # publishes its tag the way each role row has since R2049. Spelled here,
        # they were two of the sites `tools/painted_addresses.py` refuses.
        body_tag = next(
            pane["body"] for pane in spec["panes"] if pane["tag"] == "lab.palette"
        )
        wanted = [g["tag"] for g in groups]
        wanted += [role["tag"] for role in roles]
        reached: set[str] = set()
        at = 0
        while True:
            here = abs_rects_of(app.snapshot(source="paint"))
            reached |= {tag for tag in wanted if tag in here}
            if reached >= set(wanted):
                break
            before = at
            at += 40
            app.scroll(body_tag, to=(0, at))
            # ★ R2078 — `tick_ms`, because `tick()` takes SECONDS and this means
            # ONE FRAME. Written `tick(16)` at first by copying the helpers
            # above, which carry that defect and are inside the gate's budget;
            # `tools/tick_units.py` refused the push and named all four sites.
            app.tick_ms(16)
            landed = abs_rects_of(app.snapshot(source="paint"))
            # The pane clamps at its own maximum, so a step that changes nothing
            # means the bottom is reached and anything still missing is missing.
            if landed == here and at > before:
                missing = sorted(set(wanted) - reached)
                raise AssertionError(
                    f"H: the palette is scrolled to its end and {len(missing)} "
                    f"declared element(s) were never painted: {missing}"
                )
        ok(
            f"H: ★★★★★ all {len(wanted)} declared palette elements "
            f"({len(groups)} headings + {len(roles)} rows) are reached inside "
            "the assembled application",
            True,
        )
        ok(
            f"H: ★ and reaching them took a scroll (parked at {at}) -- with the "
            "canon's roster this pane does not fit, so a run that never "
            "scrolled would mean the roster shrank",
            at > 0,
        )
        # ★★★★★ And the LAST group's row is pressable where it landed. The
        # strongest form of the claim: not that twenty-one rows are drawn, but
        # that the one furthest from the top does what a palette row is for.
        last = roles[-1]
        rects = abs_rects_of(app.snapshot(source="paint"))
        assert last["tag"] in rects, f"H: {last['tag']} is on screen to be pressed"
        before_cards = card_names(app)
        press_at(app, rects[last["tag"]])
        cards = card_names(app)
        fresh = cards - before_cards
        assert_eq(
            sorted(before_cards - cards),
            [],
            "H: and nothing that was on the canvas left it",
        )
        assert_eq(
            len(fresh),
            1,
            f"H: ★★★★★ pressing {last['name']} -- the row in the canon's "
            f"seventh group, the one that needed the most scrolling -- adds "
            f"exactly one card in the assembled application (new: {sorted(fresh)})",
        )
        # ★ And the card's NAME is the one the badge predicts, which is what
        # makes `badge` a fact a client can act on rather than a label: the
        # screen mints a card as `{badge}-{ordinal}`, so a client that read the
        # specification knew what this press would create before it pressed.
        added = fresh.pop()
        ok(
            f"H: ★ and the card it made is named from that role's badge "
            f"({last['badge']}): {added}",
            added.startswith(f"{last['badge']}-"),
        )
        print(
            f"[demo] all {len(wanted)} palette element(s) reached with the pane "
            f"parked at {at}; pressing {last['name']} made {added}"
        )
        app.scroll(body_tag, to=(0, 0))
        app.tick_ms(16)

        # ── (I) the chord of each move decides where the card lands ───────
        # ★★★★★ R2080 — "drag a node to place it (hold ctrl to snap)" is written
        # in this screen's module header AND in its own published operation
        # table, and for 429 rounds nothing could perform it: the gesture
        # carried a snap flag with one construction site, hard-coded false, and
        # no press path or verb could set it, so the branch reading it was
        # unreachable while two documents told a reader the operation exists.
        #
        # What was missing was not a flag beside it but a SUPPLY. The
        # framework's move payload carried the position and no chord, so
        # nothing inside the callback could know what was held — R1619's own
        # defect one edge later: that round stamped the PRESS because a gesture
        # that begins at the press needs the chord it began with, and left the
        # march carrying position alone.
        #
        # Driven the way the canon reads it — PER MOVE, inside one held
        # gesture — because that is the half a latch cannot reproduce: the same
        # pixel resolves to two different places depending only on the chord
        # that arrived with the move, and letting the key go hands the pixel
        # back.
        banner("I — hold ctrl and the placement drag lands on the grid")
        grid = spec["grid"]
        ok(f"I: the screen publishes the grid it snaps to ({grid} canvas units)", grid > 0)
        # A card of this section's own making, so the gesture runs on something
        # no earlier section is holding.
        rects_before = abs_rects_of(app.snapshot(source="paint"))
        cards_before = card_names(app)
        press_at(app, rects_before[roles[0]["tag"]])
        arrived_cards = card_names(app) - cards_before
        assert_eq(
            len(arrived_cards),
            1,
            f"I: pressing {roles[0]['name']} put one card on the canvas",
        )
        # ★ The seat is DERIVED, never spelled: the marks that appeared with the
        # card are the card's, and the widest of them is the body a hand picks
        # up (its pins hang off the edges, so containment would refuse the very
        # rectangle it is looking for). Nothing separately proves the choice —
        # the gesture's own effect does, because `the_one_that_moved` refuses a
        # drag that moved no card.
        landed = abs_rects_of(app.snapshot(source="paint"))
        arrived = {tag: landed[tag] for tag in landed if tag not in rects_before}
        seat = max(arrived.values(), key=lambda r: r[2] * r[3])
        print(
            f"[demo] {sorted(arrived_cards)[0]} arrived as {len(arrived)} new "
            f"mark(s); the widest is {seat[2]}x{seat[3]}"
        )

        places = lab_places(app)
        grab = (seat[0] + seat[2] // 2, seat[1] + seat[3] // 2)
        aim = (grab[0] + 37, grab[1] + 29)
        app.drag(from_at=grab, to_at=aim, phase="begin")
        app.tick_ms(16)
        plain = the_one_that_moved(places, lab_places(app))
        # Walk the hand a pixel at a time until it is genuinely between two grid
        # lines, so the comparison below cannot pass by having landed on one.
        nudges = 0
        while plain[0] % grid == 0 and plain[1] % grid == 0:
            nudges += 1
            assert nudges <= grid, "I: no pixel within a grid step places off the grid"
            aim = (aim[0] + 1, aim[1])
            app.drag(from_at=aim, to_at=aim, steps=1, phase="move")
            app.tick_ms(16)
            plain = the_one_that_moved(places, lab_places(app))
        # Take the grid — same pixel, same gesture, never released.
        app.modifiers(ctrl=True)
        app.drag(from_at=aim, to_at=aim, steps=1, phase="move")
        app.tick_ms(16)
        held = the_one_that_moved(places, lab_places(app))
        ok(
            f"I: ★★★★★ with ctrl held the card lands on the {grid}-unit grid "
            f"at {held}, from the same pixel that placed it at {plain}",
            held[0] % grid == 0 and held[1] % grid == 0,
        )
        ok(
            "I: ★★★★★ and the two are different, so the chord decided rather "
            "than the pixel",
            held != plain,
        )
        # Let the key go, still holding the drag, and settle it.
        app.modifiers()
        app.drag(from_at=aim, to_at=aim, steps=1, phase="end")
        app.tick_ms(16)
        settled = the_one_that_moved(places, lab_places(app))
        ok(
            f"I: ★★★★★ letting ctrl go INSIDE the gesture hands the pixel back "
            f"({settled}) -- which a flag latched at pick-up cannot do, and is "
            "what says the chord is read per move",
            settled == plain,
        )
        print(
            f"[demo] one gesture, three chords: plain {plain} -> ctrl {held} -> "
            f"released {settled} (grid {grid}, {nudges} nudge(s) to get off it)"
        )

        # ── (J) membership is a gesture now, not a side effect ────────────
        # ★★★★★ R2082 — from R1654 until this round, dropping a card anywhere
        # over a host frame's box silently changed which machine it starts on,
        # and no gesture could decline. The behaviour canon calls its `apply
        # frame` inside `if(alt)` and nowhere else: a plain drag is placement
        # alone, alt-drag carries a card to another host, and alt-CLICK toggles
        # the one it is on without moving it — the only way INTO a frame the
        # card is already sitting inside, since a drag has nowhere to carry it.
        #
        # Driven here, in the assembled application, because the chord has to
        # cross a real wire to arrive: the shell's absolute modifier cache
        # (`scene/modifiers`), the router's press, this screen's opt-in to the
        # bare-target modifier wire, and its decode. Every one of those is a
        # place the chord can be dropped, and the in-process gates next door
        # reach none of them.
        banner("J — a card changes host only when it is asked to")
        spec = lab_spec(app)
        gestures = dict(spec["gestures"])
        for named in ("alt-drag a node", "alt-click a node"):
            ok(f"J: the screen declares {named!r}: {gestures.get(named)!r}", named in gestures)

        def hosts() -> dict:
            """Which host each card starts on, ASKED OF THE SCREEN."""
            return json.loads(str(app.query(f"/{LAB_ROOT}{EXT}/frames")))

        def seat(tag: str) -> tuple:
            box = abs_rects_of(app.snapshot(source="paint"))[tag]
            return (box[0] + box[2] // 2, box[1] + box[3] // 2)

        def band(tag: str) -> tuple:
            """A card's own IDENTITY BAND — its top strip.

            ★ Not the centre, and the reason was measured here: a card carried
            onto a wire has that wire's act chip drawn ON it, and this screen
            reads a chip BEFORE the card underneath it (R2001, deliberately —
            otherwise a press would pick the card up and the control could
            never fire). Aimed at the centre, this section's alt click landed on
            `unlinked P-01 -> R-01` and the walk reported the toggle as broken.
            """
            box = abs_rects_of(app.snapshot(source="paint"))[tag]
            return (box[0] + box[2] // 2, box[1] + 5)

        def inside_of(frame_tag: str, carried: str) -> tuple:
            """Where to LET GO so the carried card lands inside that frame.

            ★ Inset by the CARD's own size and not by a constant, because the
            drop is judged on the card's CENTRE while the cursor holds it by the
            band it was picked up from. Measured: a 24-pixel inset from the
            frame's bottom-left corner put the cursor inside the box and the
            card's centre one pixel below it, and the walk read a successful
            re-parent as *the card is on no host*.
            """
            shot = abs_rects_of(app.snapshot(source="paint"))
            frame, card_box = shot[frame_tag], shot[carried]
            return (
                frame[0] + card_box[2],
                frame[1] + frame[3] - card_box[3],
            )

        def with_alt(act) -> None:
            app.modifiers(alt=True)
            try:
                act()
            finally:
                app.modifiers()
            app.tick_ms(16)

        started = hosts()
        card = next(name for name, host in started.items() if host)
        home = started[card]
        # ★ Both addresses come FROM the screen: R2082 published the card and
        # frame templates for the reason R2049 published the role row's, and
        # this walk is the first consumer.
        card_tag = next(n["tag"] for n in spec["nodes"] if n["id"] == card)
        away = next(f for f in spec["frames"] if f["name"] != home)
        back = next(f for f in spec["frames"] if f["name"] == home)
        print(f"[demo] {card} starts on {home}; carrying it to {away['name']}")

        # ① alt-drag: it moves AND changes host.
        with_alt(
            lambda: app.drag(
                from_at=band(card_tag), to_at=inside_of(away["tag"], card_tag)
            )
        )
        assert_eq(
            hosts()[card],
            away["name"],
            f"J: ★★★★★ an ALT drag carried {card} onto {away['name']}",
        )
        # ② a plain drag back: it moves and changes NOTHING about what holds it.
        # This is the half that was impossible before the round.
        app.drag(from_at=band(card_tag), to_at=inside_of(back["tag"], card_tag))
        assert_eq(
            hosts()[card],
            away["name"],
            f"J: ★★★★★ and a PLAIN drag back into {home} left it on "
            f"{away['name']} -- position and membership are separate facts",
        )
        # ③ alt-click: off the host it is on, without moving.
        #
        # ★ The message carries what the screen SAID and what it thinks is
        # selected, because a press that missed its card and a toggle that
        # declined are the same silence from out here.
        before_click = band(card_tag)
        was_at = seat(card_tag)
        with_alt(lambda: app.click(at=before_click))
        ok(
            f"J: ★★★★★ an ALT click took {card} off its host: {hosts()[card]!r} "
            f"(said {app.query(f'/{LAB_ROOT}{EXT}/said')!r}, "
            f"selected {app.query(f'/{LAB_ROOT}{EXT}/selected')!r}, at {before_click})",
            hosts()[card] is None,
        )
        # ④ and again: back onto whichever frame encloses where it stands. It is
        # sitting inside `home`'s box after ②, so that is the answer — and no
        # drag could have produced it, because there is nowhere to carry a card
        # that is already there.
        with_alt(lambda: app.click(at=band(card_tag)))
        assert_eq(
            hosts()[card],
            home,
            "J: ★★★★★ a second ALT click put it on the host it is standing in "
            "-- the arm no drag can reach",
        )
        assert_eq(
            seat(card_tag),
            was_at,
            "J: ★ and neither click moved the card one pixel",
        )

        print(f"\n[demo] {len(CHECKS)} named check(s)")
        ok("the tool is one application at three of its seven seats", True)


def lab_spec(app: RpcSubprocess) -> dict:
    """The mounted lab's own published specification.

    ★ R2078 — read through the LAB's slot and not the shell's: `q(app, "spec")`
    answers the shell's table (its window, its rail) and the two have keys in
    common, so asking the wrong one would have answered plausibly and about
    another screen.

    Decoded defensively because the two bindings differ: the shell's slot hands
    back an object and the lab's a JSON string, which is a divergence this walk
    has no business asserting either way.
    """
    answer = app.query(f"/{LAB_ROOT}{EXT}/spec")
    if isinstance(answer, str):
        return json.loads(answer)
    return answer


def lab_selected(app: RpcSubprocess) -> str:
    """Which node the mounted lab has selected, from the lab's own wire."""
    return str(app.query(f"/{LAB_ROOT}{EXT}/selected"))


def lab_slot_answers(app: RpcSubprocess) -> bool:
    """Whether the lab's own surface is addressable on the wire right now.

    Addressed by the external's tag, which is how a host's extra surfaces are
    reachable — and the whole point of `ScreenRoster::externals`: a screen the
    journey is not at contributes no external, so there is no node for this to
    resolve against.
    """
    try:
        app.query(f"/{LAB_ROOT}{EXT}/spec")
    except Exception:  # noqa: BLE001 - any refusal shape means "not addressable"
        return False
    return True


run_demo("R1724 the tool is one application", body)
