#!/usr/bin/env python3
"""R1725 §5.16 §5.38 §2 #7 — **one application has one navigation.**

R1724 mounted a whole screen at one of this application's seats. The first
thing that mount made visible was a defect neither screen had on its own: at
the node lab destination the shell's navigation rail ran x=0..52 and the mounted
screen painted **its own** at x=52..106 — two rails, side by side, for one
application — and the accessibility tree published both of them, `role
navigation`, named *Destinations* and *sections*.

Neither screen was wrong. A screen that has ever run standalone *needs* a
navigation, and that need stops being true the moment it is placed inside an
application that has one. That is a fact about **the place**, and nothing
carried it. `pinion_core::chrome` is that fact: the host states what it
provides, for the duration of building the guest's scene, and the guest leaves
out what is already there.

What this script drives, and why it needs TWO processes:

* **A** — the integrated application at the node lab seat. One rail painted, one
  navigation in the tree, and the guest's panes shifted into the room its own
  rail no longer takes.
* **B** — the SAME binding as its own window. Its rail is back, because there
  it is the only navigation there is. This is the half a single-process test
  cannot show, and the direction that must not regress: the repair must not
  take the rail away from the standalone screen.
* **C** — the host's navigation still works with a whole other screen showing.

Measured at 6.11.1 by building a probe and running it: a complete application
window placed inside another application's page container keeps its menu bar
(23 px of it), its tool bar and its status bar; its accessibility tree carries
**2 menu bars, 2 tool bars and 2 status bars**; and there is no property,
method or event by which the guest could ask what its container provides — the
nearest signal is `window()`, which answers the *host's* window, so a guest can
learn that it is embedded and nothing at all about what that place has.

Run from the workspace root:
    cargo build -p hello-analyzer-shell -p hello-node-lab --release
    python3 tools/demos/r1725_one_application_has_one_navigation.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

SHELL = "hello-analyzer-shell"
LAB = "hello-node-lab"
EXT = "/external"
CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def nodes_by_tag(app: RpcSubprocess) -> dict:
    return {n["tag"]: n for n in app.request("scene/access").result["nodes"]}


def navigations(app: RpcSubprocess) -> list:
    return sorted(
        n["tag"] for n in app.request("scene/access").result["nodes"] if n.get("role") == "navigation"
    )


def body() -> None:  # noqa: PLR0915 - one narrative, read top to bottom
    # ── (A) the integrated application ────────────────────────────────────
    banner("A — the application at the node lab: one rail, one navigation")
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", "lab")
        app.tick_ms(16)
        assert_eq(app.query(f"{EXT}/nav"), "lab", "A: the journey reached the node lab")

        rects = abs_rects_of(app.snapshot(source="paint"))
        # ★ R2051 — the address a rail seat is painted under, recovered from one
        # the application publishes rather than spelled here.
        published = app.query(f"{EXT}/spec")["rail"]
        seat_tag = published[0]["tag"][: -len(published[0]["key"])]
        rails = sorted(t for t in rects if t in ("shell.rail", "lab.rail"))
        assert_eq(
            rails,
            ["shell.rail"],
            "A: exactly ONE rail is painted, and it is the application's. "
            "Before this round the mounted screen painted a second one "
            "immediately beside it",
        )
        ok("A: the host's rail is still there", "shell.rail" in rects)
        # Every destination the application declares still has its seat: the
        # repair must not have cost the host anything either.
        # ★ R1728 — the reference's eight, rather than the seven this had.
        for seat in (
            "dashboard",
            "packets",
            "keys",
            "logs",
            "lab",
            "topology",
            "sessions",
            "settings",
        ):
            ok(f"A: the host's {seat} seat is painted", f"{seat_tag}{seat}" in rects)
        ok(
            "A: none of the guest's rail seats is painted",
            not [t for t in rects if t.startswith("lab.rail.")],
        )
        # ★ R1725 answered only the NAVIGATION question and asserted the guest
        # kept its own bar, "so a later round cannot quietly fold them together
        # without this failing". The assertion did exactly its job: R1822 folded
        # them, and this went red.
        #
        # 🟥🟥🟥 ★★★★★ R1825 — **and the later round was right**, which is the
        # only reason this assertion is being turned over rather than the code.
        # R1725's argument was that a host's bar carries the application's
        # subject and a page's carries the page's, so they are different
        # sentences. Its OWN round had already measured the behaviour canon and
        # found one bar, the host's, on all three screens — with the graph's
        # name and run state not in it but on the canvas toolbar, which is
        # exactly where this screen already draws them. Measurement and prose
        # diverged under one hand and the code kept the prose for 97 rounds.
        #
        # ⇒ the assertion is INVERTED rather than deleted, because the property
        # it guards is still the one that matters: a later round must not be
        # able to give the guest its bar back without saying so here.
        ok("A: the host's own bar is painted", "shell.appbar" in rects)
        ok(
            "A: ★★★★★ and the guest draws NO bar of its own -- one application, "
            "one bar, which is what the behaviour canon has on all three of its "
            "screens",
            "lab.appbar" not in rects,
        )
        # ★★★★★ And the graph's name is still SAID. Dropping the strip is only
        # half the move: the toolbar's copy of the name deferred to the bar with
        # a silence, and a silence is a REFERENCE — left alone it points at a
        # node that is not in the tree and the name is painted and announced by
        # nobody. R1822 stopped at "not deferring", which is `unvoiced`, and the
        # census caught it two rounds later. Asserted here on the ASSEMBLED
        # application, which is the only place the mounted arrangement exists.
        tree = nodes_by_tag(app)
        ok(
            "A: ★★★★★ and the graph's name is still announced by exactly one "
            "stop -- the toolbar's copy, which the strip's removal must not "
            "have left saying nothing",
            "lab.appbar" not in tree
            and bool(tree.get("lab.toolbar.title", {}).get("name")),
        )

        # 🟥🟥🟥 ★★★★★ R1825 — **a mounted guest must hit-test the screen it
        # painted**, and this is the assertion whose absence cost three demos.
        #
        # What a host provides is stated in a SCOPE around the build. Every hook
        # the framework calls on the guest afterwards — what is under a pointer,
        # where a press lands, what the wheel does — runs outside that scope,
        # and outside it the declaration read `NONE`, which is not "no answer"
        # but *you are standalone*. So this screen laid its panes out without
        # the application bar and hit-tested them with it. Measured here before
        # the repair: **41 regions addressed a DIFFERENT region at their own
        # centre**, and 0 did so standalone at the same size. No denominator:
        # the count of painted regions moves with what is scrolled into view,
        # so it is a fact about the frame rather than about the defect.
        #
        # `astray` is the arm that says exactly that, and nothing was asserting
        # it: the harness's boot gate refuses only `unreachable`, and it runs
        # before this demo has navigated anywhere.
        #
        # ⚠ Judged at the size this demo boots at, which is the limit R1742
        # states about its own census: a screen can agree with itself at one
        # size and not another. One size gated beats none, and the caveat is
        # written rather than left for a reader to infer.
        census = app.request("scene/pointer_target").result
        guest = [s for s in census["surfaces"] if s["surface"] == "node_lab"]
        ok("A: the mounted guest answers the pointer census at all", len(guest) == 1)
        assert_eq(
            guest[0]["astray"],
            0,
            "A: ★★★★★ no region of the mounted guest addresses a DIFFERENT "
            "region at its own centre -- the paint and the hit test read one "
            "set of facts about the place this screen was put in",
        )

        # ★★★★★ The half a picture cannot show. Omitting the paint alone would
        # leave a landmark a screen reader walks to and a pointer cannot reach.
        assert_eq(
            navigations(app),
            ["shell.rail"],
            "A: the accessibility tree publishes ONE navigation -- the row the "
            "reference toolkit fails, where a placed window's bars stay in the "
            "tree and a reader is told the application has two of each",
        )

        # ★ The room is USED, not left blank: this is the difference between
        # omitting a pane and merely not drawing it.
        page = rects.get("window.pan") or rects["shell.canvas"]
        palette = rects["lab.palette"]
        assert_eq(
            palette[0],
            page[0],
            "A: the guest's palette starts at the page's own left edge, in the "
            "room the rail it no longer draws used to take",
        )
        ok("A: the guest still paints its screen", len(
            [t for t in rects if t == "node_lab" or t.startswith("lab.")]) > 40)
        ok("A: including its canvas", "lab.canvas" in rects)
        ok("A: and its inspector", "lab.inspector" in rects)

        # ★ R1825 — the SECOND site that asserted the guest keeps its bar, and
        # it is worth noting that it exists: turning over the first one left
        # this one standing, and a demo whose whole subject is "one application
        # has one X" had written the claim twice. The room the strip took is
        # what the assertion is really about, so it now checks that instead of
        # checking the strip is there.
        ok("A: the guest draws no bar of its own", "lab.appbar" not in rects)
        ok(
            "A: and its content starts at the page's own top edge, in the room "
            "the strip used to take",
            rects["lab.palette"][1] == page[1],
        )

        # ── (C) the host's navigation still navigates ─────────────────────
        banner("C — the application's own rail still works")
        seat = rects[f"{seat_tag}dashboard"]
        app.request(
            "scene/click",
            {"button": "left", "at": {"x": seat[0] + seat[2] // 2, "y": seat[1] + seat[3] // 2}},
        )
        app.tick_ms(16)
        assert_eq(app.query(f"{EXT}/nav"), "dashboard", "C: the rail took us to the dashboard")
        after = abs_rects_of(app.snapshot(source="paint"))
        ok(
            "C: and the guest left with it",
            not [t for t in after if t == "node_lab" or t.startswith("lab.")],
        )
        app.intervene(f"{EXT}/nav", "lab")
        app.tick_ms(16)
        back = abs_rects_of(app.snapshot(source="paint"))
        ok("C: returning brings the guest back", "lab.canvas" in back)
        ok(
            "C: and it still has no rail of its own",
            "lab.rail" not in back,
        )
        assert_eq(
            navigations(app),
            ["shell.rail"],
            "C: and the tree still publishes one navigation after a round trip "
            "-- the declaration is per frame, so a second visit is a second "
            "chance to get it wrong",
        )

    # ── (B) the same binding, as its own window ───────────────────────────
    banner("B — the SAME screen standalone: its rail is back")
    with RpcSubprocess(LAB, boot_grace=1.5) as lab:
        rects = abs_rects_of(lab.snapshot(source="paint"))
        ok("B: standalone, the screen draws its own rail", "lab.rail" in rects)
        seats = sorted(t for t in rects if t.startswith("lab.rail."))
        ok(f"B: with all {len(seats)} of its seats", len(seats) >= 7)
        assert_eq(
            rects["lab.rail"][0],
            0,
            "B: at the window's own left edge",
        )
        assert_eq(
            rects["lab.palette"][0],
            rects["lab.rail"][0] + rects["lab.rail"][2],
            "B: with the palette beside it, exactly where it always was -- the "
            "standalone layout is unchanged, which is what makes this a "
            "statement about the PLACE rather than an edit to the screen",
        )
        assert_eq(
            navigations(lab),
            ["lab.rail"],
            "B: and standalone it is the one navigation there is",
        )
        tree = nodes_by_tag(lab)
        ok(
            "B: its seats are in the tree too",
            len([t for t in tree if t.startswith("lab.rail.")]) >= 7,
        )
        # ★ And the rest of the screen is the same screen in both places, which
        # is what makes this a statement about the PLACE rather than an edit:
        # every pane it paints hosted, it paints standalone.
        for pane in ("lab.palette", "lab.canvas", "lab.inspector", "lab.appbar"):
            ok(f"B: standalone it still paints {pane}", pane in rects)
        assert_eq(
            navigations(lab),
            ["lab.rail"],
            "B: and one navigation is one navigation here too -- the repair is "
            "not 'the guest never has a rail', it is 'the guest has one where "
            "nothing else does'",
        )

        # ★★★★★ WHY DELETING IT WAS SAFE, driven rather than read. The claim
        # that made this round's deletion honest is that the guest's rail
        # navigates NOWHERE — so omitting it inside the application costs no
        # capability. That is a claim about behaviour, so it is pressed: every
        # seat answers with a refusal naming itself, and the screen it is on
        # does not change.
        before = lab.query(f"{EXT}/spec")
        for seat in ("packets", "keys", "logs"):
            rect = rects[f"lab.rail.{seat}"]
            lab.request(
                "scene/click",
                {
                    "button": "left",
                    "at": {"x": rect[0] + rect[2] // 2, "y": rect[1] + rect[3] // 2},
                },
            )
            lab.tick_ms(16)
            said = lab.query(f"{EXT}/toast")
            ok(
                f"B: pressing {seat} is refused, and says so ({said!r})",
                seat in str(said),
            )
        assert_eq(
            lab.query(f"{EXT}/spec"),
            before,
            "B: and none of those presses moved the screen anywhere -- the "
            "guest's rail is an affordance that refuses, which is exactly why "
            "leaving it out inside a host that HAS a navigation costs nothing",
        )

    print(f"\n[demo] {len(CHECKS)} named check(s)")
    ok("one application has one navigation, and a screen alone still has its own", True)


run_demo("R1725 one application has one navigation", body)
