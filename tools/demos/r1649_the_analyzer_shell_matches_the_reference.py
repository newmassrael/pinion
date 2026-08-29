#!/usr/bin/env python3
"""R1648/R1649/R1668 §5.21 §5.51 §5.39 §2 #7 — the analysis-tool dashboard
shell, assembled, operated by hand, and shaped like the tool this axis is
judged by.

`tools/analyzer_census.py` gives every capability of this tool class one of five
verdicts, and the biggest bin is `app` — *the substrate is here, the domain
logic is the application's*. That is a claim about COMPOSITION, and the only
thing that proves a composition is a composite. `hello-analyzer-shell` is that
composite and this script is what makes the claim checkable; the census's `app`
rows name both in `assembled_by`.

R1649 rebuilt the shell against the reference tool's own screen and gestures,
because "the substrate can express it" is not the same claim as "the substrate
can express it the way a professional tool does". What that cost, and what this
script therefore checks:

* **A three-column shell** — icon rail, sub-header with the layout preset and
  the two board verbs, canvas, and a **widget palette the board is populated
  from**. Thirteen kinds offered — four this release places and nine it
  RESERVES — and the counts on screen have to agree.
* **R1668: a reserved seat is present, named, and says what it is waiting for.**
  The reference is emphatic that a later release's widgets are shown locked
  rather than hidden, so the shape of the finished tool is legible before it
  exists. Each one is declared unavailable with the requirement it is booked
  under, which is what puts the reason on `scene/disabled`, in the accessibility
  tree, and out of reach of every path that places a card. Measured on the
  toolkit at 6.11 by building and running it: four disable surfaces, all bools,
  and one accessibility bit with no slot for a reason.
* **A drag previews where it will land** rather than displacing cards live. The
  reference commits on release; so does this, and `drag` publishes the snap.
* **Layout-edit mode** puts size steppers on the cards.
* **Detach REMOVES the card from the board** and re-dock appends it at the
  bottom. R1648 kept the tile in place and argued for it; the reference does
  not, and on this axis the reference is the specification.
* **A header is a set, and the set is enforced on the wire** — an affordance a
  card does not offer is refused by name, because hiding a button leaves an
  agent able to do what the screen says is impossible.
* **A body state carries its own derived remedy.** The two that look identical
  on screen — a permission denial and an encrypted link — are opposite in what
  a person can do. Measured on the toolkit at 6.11: it has no content-state
  concept at all.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1649_the_analyzer_shell_matches_the_reference.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import (  # noqa: E402
    opening_board,
    opening_kinds,
    owed_keys,
    palette_bookings,
)
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_declared_channels_are_true,
    assert_eq,
    assert_router_press_moves,
    find_by_tag,
    resize_and_settle,
    run_demo,
    texts_of,
    walk_nodes,
)

EXT = "/external"

#: The thirteen catalogue kinds, in palette order. Read back from the running
#: application's own `spec` below as well — this list is what makes a silent
#: reordering visible, since a comparison against the application alone would
#: agree with whatever the application happened to say.
CATALOGUE = [
    "packet",
    "decode",
    "keymap",
    "filter",
    "topology",
    "overlay",
    "throughput",
    "share",
    "latency",
    "health",
    "loss",
    "alarms",
    "admin",
]

#: The board this release opens with, DERIVED from the specification.
#:
#: ★ R1797 — `latency#4` joined them. Its seat was booked under the reference's
#: second release for as long as the framework could not draw what sits in it:
#: a latency distribution needs geometric buckets with an unbounded tail, and
#: the histogram had neither until that round.
#:
#: ★★★★★ R1846 — and R1843 promoted `health#5` the same way, at which point
#: this line — a literal five — FAILED FOR A CHANGE THAT WAS THE RELEASE PLAN
#: WORKING, and stayed failing for two rounds because nothing ran it. The board
#: is exactly the kind of fact that moves, so it is read from
#: `docs/analyzer-dashboard-spec.json` now, which is a document written by
#: another hand and is what makes the comparison below a comparison. The
#: ORDER pin is [`CATALOGUE`] above and stays a literal, because order is what
#: a literal can honestly hold.
OPENING = opening_board()

#: What a later release still books, and the requirement each is booked under,
#: in PALETTE order.
#:
#: ★★★★★ R1846 — derived as *deferred minus built*, because a seat leaves this
#: list by being BUILT: `docs/analyzer-reserved-spec.json` records `latency`
#: (R1797) and `health` (R1843) in `built` while requirement 18 stays in
#: `deferred`, since the rail's `sessions` seat cites the same requirement and
#: is still locked. Written out by hand this held `health` for two rounds after
#: the card was on the board.
#:
#: Ordered by [`CATALOGUE`] rather than by the register, because the assertion
#: below compares against the palette the screen draws — the register's order
#: is the order requirements were written in, which is a different fact.
RESERVED = {
    kind: booking
    for kind, booking in sorted(
        palette_bookings().items(), key=lambda pair: CATALOGUE.index(pair[0])
    )
}

#: ★ R1797 — the latency card's own numbers, as the reference publishes them.
#: The application derives all three from one capture record; these are the
#: oracle that derivation is checked against over the wire.
LATENCY_TILES = {"p50": "3.2 ms", "p95": "11.4 ms", "max": "72.0 ms"}

#: The reference's bucket ladder, and the two open ends that make it eight.
LATENCY_BUCKETS = ["<1", "1-2", "2-4", "4-8", "8-16", "16-32", "32-64", ">64"]


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def refused(tf: RpcSubprocess, path: str, args) -> str:
    try:
        inv(tf, path, args)
    except Exception as why:  # noqa: BLE001 - any refusal shape is fine here
        return str(why)
    raise AssertionError(f"{path}({args!r}) was expected to be refused")


def refused_write(tf: RpcSubprocess, path: str, value) -> str:
    try:
        tf.intervene(f"{EXT}/{path}", value)
    except Exception as why:  # noqa: BLE001
        return str(why)
    raise AssertionError(f"a write to {path} was expected to be refused")


def click(tf: RpcSubprocess, where: str) -> str:
    """Press and release at `where`, the way a hand does."""
    inv(tf, "point", where)
    inv(tf, "send", "PointerDown")
    return inv(tf, "send", "PointerUp")


def centre(rect: dict) -> str:
    return f"{rect['x'] + rect['w'] // 2},{rect['y'] + rect['h'] // 2}"


def paint(tf: RpcSubprocess):
    return tf.snapshot(source="paint", viewport=(1440, 900))


def at(tf: RpcSubprocess, tag: str) -> str:
    """The point at the centre of the rectangle `tag` was PAINTED in.

    Asking the scene rather than recomputing the shell's layout here: a demo
    carrying a second copy of the geometry could not notice a drift between the
    painter and the hit test, which is the whole property section (K) holds.
    """
    # ★ R1664 — WINDOW-absolute, via `abs_rects_of`. A node inside a `Scroll`
    # carries a scroll-LOCAL rect, and R1662 made this shell's board a scroll
    # pane — so `find_by_tag(...)["rect"]` stopped being the place a pointer
    # reaches and every press aimed by this helper landed on nothing. The
    # docstring above was still true of the intent and false of the code.
    rects = abs_rects_of(paint(tf))
    assert tag in rects, f"{tag} is painted"
    x, y, w, h = rects[tag]
    return centre({"x": x, "y": y, "w": w, "h": h})


def cell_of(grid: dict, cid: str) -> tuple[int, int]:
    return next((t["col"], t["row"]) for t in grid["tiles"] if t["id"] == cid)


def row_of(grid: dict, cid: str) -> int:
    return next(t["row"] for t in grid["tiles"] if t["id"] == cid)


def body() -> None:
    with RpcSubprocess("hello-analyzer-shell", boot_grace=1.5) as tf:
        counted = assert_declared_channels_are_true(tf)
        assert counted["read"] >= 25, f"the walk reaches the surface: {counted}"

        # ── (A) the shell is assembled: a catalogue, a board, and a count ─
        assert_eq(q(tf, "catalogue"), ",".join(CATALOGUE), "A: thirteen kinds offered")
        assert_eq(
            q(tf, "cards"),
            ",".join(OPENING),
            f"A: the specification's {len(OPENING)} placed, in board order",
        )
        assert_eq(q(tf, "placed_count"), len(OPENING), "A: and the count agrees")
        assert_eq(q(tf, "preset"), "Overview", "A: the layout it opens on")
        assert_eq(
            q(tf, "rail"),
            "dashboard,packets,keys,logs,lab,topology,sessions,settings",
            "A: ★ R1728 -- the reference's EIGHT seats, in the reference's own "
            "order. This read seven until this round, three of whose keys the "
            "reference does not have",
        )
        assert_eq(q(tf, "tabs"), "Dashboard,Design System", "A: the two view tabs")
        assert q(tf, "source").startswith("eth0"), "A: the app bar opens on a source"
        assert_eq(q(tf, "capturing"), True, "A: capture is on")
        assert_eq(q(tf, "theme"), "dark", "A: and the shell opens dark, as the reference does")
        assert "is not a source" in refused_write(tf, "source", "nope"), (
            "A: a source outside the offered set is refused BY NAME"
        )
        assert "is not a tab" in refused_write(tf, "tab", "Elsewhere"), "A: closed set"
        assert "is not a rail section" in refused_write(tf, "nav", "nowhere"), "A: closed set"

        # ── (B) ★ a card is ADDED from the palette, not seeded ───────────
        # The reason the palette exists: what is on the board is a decision
        # somebody made, so two places on screen have to agree about how many.
        # ★★★★★ R1846 — every ordinal below is DERIVED from the board's size.
        # They were literals, and every one of them was a second recording of
        # how many cards this release opens with: promoting a sixth card moved
        # `packet#5` to `packet#6` and each count by one, so a demo that was
        # already failing on line one would have failed nine more times on the
        # way down. `next_id` is the first free ordinal — ids ascend and are
        # never reused, which is the fact these numbers actually rest on.
        next_id = len(OPENING)
        assert_eq(
            inv(tf, "add", "packet"),
            f"packet#{next_id}",
            "B: a new card takes the next ordinal",
        )
        assert_eq(q(tf, "placed_count"), next_id + 1, "B: and the board grew")
        assert "is not a widget kind" in refused(tf, "add", "nonesuch"), (
            "B: the catalogue is closed, and the refusal lists it"
        )
        # A kind can be placed twice — which is why an id carries an ordinal.
        assert_eq(
            inv(tf, "add", "packet"), f"packet#{next_id + 1}", "B: twice is allowed"
        )
        assert_eq(q(tf, "placed_count"), next_id + 2, "B: two more on the board")
        # Closed over the wire rather than by hand: the fifth card lands below
        # the viewport, and this shell does not scroll its canvas
        # (debt-the-analyzer-canvas-does-not-scroll). Stated here rather than
        # worked around silently, because a demo that quietly avoided the edge
        # of the window would be hiding the difference from the reference.
        inv(tf, "act", f"packet#{next_id + 1},close")
        assert_eq(q(tf, "placed_count"), next_id + 1, "B: and one closed")
        # And placing one from the PALETTE, which is the gesture that matters.
        # ★ The ordinal keeps climbing past the id just closed — they are not
        # reused, which is what makes an id a name rather than a slot number.
        click(tf, at(tf, "shell.palette.keymap"))
        assert_eq(q(tf, "placed_count"), next_id + 2, "B: ★ the palette places a card")
        assert q(tf, "cards").endswith(f"keymap#{next_id + 2}"), (
            f"B: by kind: {q(tf, 'cards')}"
        )
        inv(tf, "act", f"keymap#{next_id + 2},close")
        assert_eq(
            q(tf, "placed_count"),
            next_id + 1,
            "B: the added card stays for what follows",
        )

        # ── (B2) ★ R1668 — the eight seats a later release opens ──────────
        # The reference shows them rather than hiding them, so the shape of the
        # finished tool is legible now. Each one says what it is waiting for,
        # and no path here places it.
        spec = q(tf, "spec")
        assert_eq(
            spec["placeable_count"],
            len(OPENING),
            "B2: what this release places is what the specification places",
        )
        assert_eq(
            spec["reserved_count"],
            len(RESERVED),
            "B2: and what it reserves is what the register still books",
        )
        assert_eq(
            [w["kind"] for w in spec["catalogue"] if w["tier"] == "reserved"],
            list(RESERVED),
            "B2: the reserved seats, in palette order",
        )
        # ★★ R1797 — a section publishes the releases its ENTRIES occupy, and
        # `visual` now holds both. Before this round a section carried a single
        # tier of its own, which is the same fact stored twice — and promoting
        # one widget out of a group made one of the two copies false.
        visual = next(s for s in spec["sections"] if s["key"] == "visual")
        assert_eq(
            visual["tiers"],
            ["placeable", "reserved"],
            "B2: ★ a mixed section says so on the wire",
        )
        assert "RELEASE 1 + 2" in visual["heading"], (
            f"B2: and the heading a reader scans says so: {visual['heading']!r}"
        )
        capture = next(s for s in spec["sections"] if s["key"] == "capture")
        assert_eq(capture["tiers"], ["placeable"], "B2: while a pure section names one")
        for kind, booking in RESERVED.items():
            entry = next(w for w in spec["catalogue"] if w["kind"] == kind)
            assert_eq(entry["reserved_for"], booking, f"B2: {kind} states its booking")
            why = refused(tf, "add", kind)
            assert booking in why, f"B2: ★ {kind} refuses AND names the booking: {why}"
        assert_eq(
            q(tf, "placed_count"),
            next_id + 1,
            "B2: and not one of them reached the board",
        )

        # ★ The same fact on the READ channel, from the framework's own cascade
        # rather than from anything this shell wrote down: `scene/disabled` says
        # which seats are inert, why, and what a person can do about it. The
        # floor this is measured against has a bool on four surfaces and one
        # accessibility bit with no slot for a reason.
        inert = {row["tag"]: row for row in tf.request("scene/disabled", {}).result["disabled"]}
        for kind, booking in RESERVED.items():
            row = inert.get(f"shell.palette.{kind}")
            assert row is not None, f"B2: shell.palette.{kind} is reserved and reported live"
            assert_eq(row["reason"], "reserved", f"B2: {kind} is inert as a reservation")
            assert_eq(row["detail"], booking, f"B2: {kind} reports its booking on the wire")
            assert_eq(row["recourse"], "await_release", f"B2: {kind} derives its recourse")
        # ★ R1846 — the placeable kinds, derived. Written out by hand this said
        # four and had said four since R1668, so neither `latency` nor `health`
        # was ever checked for being live: the loop passed by not looking.
        for kind in set(opening_kinds()):
            assert f"shell.palette.{kind}" not in inert, f"B2: {kind} is placeable and live"
        # ★ R1728 — requirement 18, not 14. The reference names the requirement
        # in the seat's own tooltip, and 14 is not among the six it defers.
        for seat, booking in (("topology", "requirement 12"), ("sessions", "requirement 18")):
            row = inert.get(f"shell.rail.{seat}")
            assert row is not None, f"B2: the {seat} rail seat is reserved and reported live"
            assert_eq(row["detail"], booking, f"B2: and names what it waits for")
        # ★ R1695 — a SECOND reason joined, and the split is exact: everything
        # inert here is either booked for a later release or built on another
        # surface of this product, and the three that are the second kind are
        # rail destinations this application cannot take you to. Before that
        # round they were painted live and refused nothing.
        elsewhere = [
            row["tag"]
            for row in tf.request("scene/disabled", {}).result["disabled"]
            if row["reason"] == "elsewhere"
        ]
        # ★★★★★ R1724 — **three, then two.** The node lab's page is mounted
        # whole through `pinion_screen::Mount<NodeLabView>`, so that seat
        # stopped saying *built, shipping, and not here* — it is here.
        # ★★★★★ R1728 — **two, then one.** The other two were never one screen
        # behind two seats: measured against the reference, `stream` and
        # `decode` were this application's invention.
        # ★★★★★ R1729 — **one, then NONE.** The capture viewer is mounted, so no
        # seat of this rail is built-and-not-here any more. In the Rust source
        # the arm itself is gone (nothing constructed it, and an unconstructed
        # variant is a compile error under this workspace's lints); on the wire
        # the vocabulary survives, because it is the framework's, so what this
        # asserts is that this SCREEN never uses it.
        assert_eq(
            sorted(elsewhere),
            [],
            "B2: every built section of this tool is reachable from this one "
            "application",
        )
        # ★★★★★ R1728 — and a THIRD reason joined, for the two seats the
        # reference has working and this build has not written. Neither existing
        # word fits: *reserved* would claim they are deferred to a later release
        # when they are in this one, and *elsewhere* would send a reader to open
        # a window nobody has built. A seat that cannot say why it is inert gets
        # left off the rail, which is exactly what had happened to both of them.
        unbuilt = [
            row["tag"]
            for row in tf.request("scene/disabled", {}).result["disabled"]
            if row["reason"] == "unbuilt"
        ]
        # ★ R1730 — DERIVED from `docs/analyzer-rail-spec.json`. Written out,
        # this named both sections, and the round that BUILT one of them left
        # this demo asserting the build was worse than it is.
        assert_eq(
            sorted(unbuilt),
            [f"shell.rail.{key}" for key in owed_keys()],
            "B2: the sections specified and not built say so, and they are "
            "exactly the ones the specification declares owed",
        )
        for tag in unbuilt:
            row = next(
                r
                for r in tf.request("scene/disabled", {}).result["disabled"]
                if r["tag"] == tag
            )
            assert_eq(
                row["recourse"],
                "await_release",
                f"B2: {tag} derives waiting -- the same ACTION a reserved seat "
                "asks for, from a different reason, which is why the kinds stay "
                "apart while the recourse merges",
            )
        assert_eq(
            [
                row["tag"]
                for row in tf.request("scene/disabled", {}).result["disabled"]
                if row["reason"] not in ("reserved", "unbuilt")
            ],
            [],
            "B2: nothing on this screen is inert for any other reason",
        )
        # And the rail refuses on its own channel, by name.
        assert "reserved for requirement 12" in refused_write(tf, "nav", "topology"), (
            "B2: ★ a reserved rail seat refuses the write and says why"
        )
        assert_eq(q(tf, "nav"), "dashboard", "B2: and the section did not change")

        # ── (B3) ★★ R1671 — the screen and its GESTURE agree about the window
        # after a resize, driven over the wire because that is the only path
        # that exercises it. The Rust sweep sets the shell's size signal
        # directly, so both halves of it read one value in-process and four
        # counterfactuals passed against it; the defect lived on the External's
        # invoke path, which runs with no `Owner` scope and so could not read
        # the size at all. A person maximising the window is what found it, and
        # this is that person, written down.
        # ★ R1686 — settled, not counted. Three ticks is a bigger bet than one
        # and still a bet: this reads the paint back and asserts against the new
        # window, so a render that has not arrived answers with the OLD
        # rectangles and the assertion below fails for a reason that has nothing
        # to do with what it is testing.
        big = (2494, 1531)
        resize_and_settle(tf, big)
        grown = abs_rects_of(paint(tf))
        palette = grown["shell.palette"]
        assert palette[0] + palette[2] == big[0], (
            f"B3: the palette's right edge is {palette[0] + palette[2]} in a "
            f"{big[0]}px window — the PAINT did not follow the resize"
        )
        # And every control the paint moved answers for itself where it landed.
        misses = []
        for kind in CATALOGUE:
            tag = f"shell.palette.{kind}"
            x, y, w, h = grown[tag]
            got = inv(tf, "point", f"{x + w // 2},{y + h // 2}")
            if got != tag:
                misses.append((tag, got))
        assert not misses, (
            f"B3: ★ {len(misses)} control(s) moved by the resize now answer as "
            f"something else: {misses[:4]} — the paint follows the window and "
            f"the gesture does not, which is what a person sees as 'nodes stop "
            f"clicking after a maximise'"
        )
        resize_and_settle(tf, (1440, 900))

        # ── (C) ★ a header is a set, and the set is enforced ─────────────
        assert_eq(
            q(tf, "affordances"),
            "settings,tear_off,maximize,close",
            "C: the four affordances, published in layout order",
        )
        assert_eq(
            inv(tf, "chrome", "packet#0"),
            "settings,tear_off,maximize,close",
            "C: ★ ONE card offering tear-off AND maximise — the toolkit splits "
            "those across two class hierarchies that cannot be combined, so a "
            "card with both is not expressible there at all",
        )
        # ★ R1668 — the chrome is UNIFORM across the board, which is what the
        # reference does and what makes a missing control legible. R1649 had it
        # vary between kinds so that a refusal could be demonstrated at all;
        # that case moved to the reserved kinds in (B2), which refuse for a
        # reason a person can read rather than for an accident of a table.
        for card in q(tf, "cards").split(","):
            assert_eq(
                inv(tf, "chrome", card),
                "settings,tear_off,maximize,close",
                f"C: {card} carries the same four as every other card",
            )
        # ★ R1846 — addressed at the card section (B) added, not at a literal
        # ordinal: `packet#5` was the id that section produced when the board
        # opened with five cards, so this line was silently asking about a card
        # that does not exist the moment a sixth was promoted — and a refusal
        # for the WRONG reason ("no such card") would still contain neither of
        # the words asserted, so the check would have gone on passing for a
        # reason it was not written for if the wording had been looser.
        assert "is not an affordance" in refused(tf, "act", f"packet#{next_id},float"), (
            "C: closed set"
        )
        assert "is not <card>,<affordance>" in refused(tf, "act", f"packet#{next_id}"), (
            "C: malformed"
        )

        # ── (D) ★ the body state, and its DERIVED remedy ─────────────────
        assert_eq(
            q(tf, "states"),
            "ready,loading,empty,failed,denied,opaque",
            "D: six states — the capability list's five reasons there is no "
            "content, plus content",
        )
        assert_eq(q(tf, "remedies"), "wait,retry,widen,authorize,nothing", "D: five remedies")
        for word, card, detail, remedy, actionable in [
            ("loading", "decode#1", None, "wait", "no"),
            ("empty", "keymap#2", None, "widen", "yes"),
            ("failed", f"packet#{next_id}", "collector unreachable", "retry", "yes"),
        ]:
            arg = f"{card},{word}" if detail is None else f"{card},{word},{detail}"
            inv(tf, "set_state", arg)
            assert_eq(inv(tf, "state", card), word, f"D: {card} is {word}")
            assert_eq(inv(tf, "remedy", card), remedy, f"D: which derives {remedy}")
            assert_eq(inv(tf, "actionable", card), actionable, f"D: actionable={actionable}")
        inv(tf, "set_state", "packet#0,denied,operator role")
        assert_eq(inv(tf, "detail", "packet#0"), "operator role", "D: a denial names the right")
        assert_eq(
            (inv(tf, "remedy", "packet#0"), inv(tf, "actionable", "packet#0")),
            ("authorize", "yes"),
            "D: ★ a denial is actionable — somebody holds the right",
        )
        inv(tf, "set_state", "packet#0,opaque")
        assert_eq(
            (inv(tf, "remedy", "packet#0"), inv(tf, "actionable", "packet#0")),
            ("nothing", "no"),
            "D: ★★ and an encrypted link is NOT, though both render as 'no "
            "content'. Collapsing them into one `error` arm is what makes a "
            "shell offer 'request access' on a link no permission can open",
        )
        assert_eq(inv(tf, "detail", "packet#0"), "", "D: and it carries no particular reason")
        assert "carries a reason" in refused(tf, "set_state", "packet#0,failed"), (
            "D: a failure with no reason is a failure whose reason was lost"
        )
        assert "carries no reason" in refused(tf, "set_state", "packet#0,empty,because"), (
            "D: and a reason on an arm nothing reads it from is refused too"
        )
        assert "is not a card state" in refused(tf, "set_state", "packet#0,broken"), "D: closed"
        inv(tf, "set_state", "packet#0,ready")

        # ── (E) ★ a drag PREVIEWS, and commits on release ────────────────
        assert_eq(q(tf, "drag"), "", "E: nothing is being dragged")
        before = json.loads(q(tf, "layout"))
        # ★★★★★ R1846 — the start cell is READ, not pinned. It was `(0, 2)`, and
        # R1843 promoting a sixth card reflowed the board so the card starts
        # somewhere else — a literal here encodes the whole board's arrangement
        # in a line about one drag, and fails for any change to the arrangement
        # whether or not the drag still works. What this section is FOR is that
        # the drag previews and commits, and both assertions below are stronger
        # read this way: `drag` and `layout` are two separate wire answers, so
        # comparing them asks whether the drag opened on the cell the board says
        # the card is in, rather than whether both match a number typed here.
        start = cell_of(before, "keymap#2")
        assert start is not None, "E: the board says where the card starts"
        inv(tf, "point", at(tf, "card.keymap#2.grip"))
        inv(tf, "send", "PointerDown")
        assert_eq(
            q(tf, "drag"),
            f"keymap#2,{start[0]},{start[1]}",
            "E: the drag opens on its own cell",
        )
        inv(tf, "point", "760,620")
        snap = q(tf, "drag")
        assert snap.startswith("keymap#2,") and snap != f"keymap#2,{start[0]},{start[1]}", (
            f"E: the snap preview follows the cursor: {snap}"
        )
        assert_eq(
            json.loads(q(tf, "layout")),
            before,
            "E: ★ and the BOARD HAS NOT MOVED — the reference commits on "
            "release, and a board that reflowed under the finger would make the "
            "preview a lie",
        )
        preview = tuple(int(n) for n in snap.split(",")[1:])
        inv(tf, "send", "PointerUp")
        assert_eq(q(tf, "drag"), "", "E: the drag is over")
        assert_eq(
            cell_of(json.loads(q(tf, "layout")), "keymap#2"),
            preview,
            "E: ★ and the card landed exactly where the preview said",
        )

        # ── (F) ★ layout-edit mode, and the size steppers ────────────────
        assert_eq(q(tf, "editing"), False, "F: the board is locked to start")
        assert_eq(q(tf, "steppers"), "narrow,widen,shorter,taller", "F: four size steps")
        assert "is not a size step" in refused(tf, "resize", "keymap#2,sideways"), "F: closed set"
        click(tf, at(tf, "shell.subbar.edit"))
        assert_eq(q(tf, "editing"), True, "F: pressing Edit Layout turns it on")
        was = inv(tf, "cell", "keymap#2")
        # The size it starts at comes from the application's own specification,
        # so widening and narrowing has to return exactly there. Writing the
        # number down here would make this pass on whatever the shell happens
        # to open with.
        opening = next(p for p in spec["board"] if p["kind"] == "keymap")
        click(tf, at(tf, "card.keymap#2.widen"))
        assert inv(tf, "cell", "keymap#2") != was, "F: ★ a stepper resized the card by hand"
        assert_eq(
            inv(tf, "resize", "keymap#2,narrow"),
            f"{opening['cols']}x{opening['rows']}",
            "F: and back, over the wire, to the size the specification gives it",
        )
        click(tf, at(tf, "shell.subbar.edit"))
        assert_eq(q(tf, "editing"), False, "F: Done turns it off")
        assert_eq(inv(tf, "key", "e"), True, "F: and `e` is the same toggle")
        assert_eq(q(tf, "editing"), True, "F: one handler, two entry points")
        inv(tf, "key", "e")

        # ── (G) ★ detach REMOVES the card, re-dock appends at the bottom ─
        assert_eq(q(tf, "floating"), "", "G: nothing is detached")
        placed = q(tf, "placed_count")
        assert_eq(inv(tf, "act", "packet#0,tear_off"), "packet#0 tear_off", "G: detach it")
        assert_eq(q(tf, "floating"), "packet#0", "G: which the shell reports")
        assert_eq(q(tf, "placed_count"), placed - 1, "G: ★ and the BOARD lost a card")
        assert_eq(q(tf, "card_count"), placed, "G: it exists, elsewhere")
        assert_eq(
            inv(tf, "cell", "packet#0"),
            "detached",
            "G: ★ so 'where is it' answers 'not on the board' rather than a cell",
        )
        assert "already detached" in refused(tf, "act", "packet#0,tear_off"), "G: once only"
        # ★ R1896 — the panel this leg goes on to click is the CANVAS one, so
        # the home is asked for. R1891 gave a detached card a home and made a
        # windowing host prefer a real window; the `float.<id>` this leg reads
        # and the re-dock control it clicks are the canvas home's paint.
        inv(tf, "detach_home", "packet#0,canvas")
        assert find_by_tag(paint(tf), "float.packet#0") is not None, (
            "G: the detached panel is painted on the canvas, with its own header"
        )
        click(tf, at(tf, "float.packet#0.redock"))
        assert_eq(q(tf, "floating"), "", "G: the re-dock control put it back")
        assert_eq(q(tf, "placed_count"), placed, "G: on the board again")
        bottom = json.loads(q(tf, "layout"))
        assert row_of(bottom, "packet#0") == max(t["row"] for t in bottom["tiles"]), (
            "G: ★ at the BOTTOM, as the reference does — a card that left the "
            "board lost its cell, and inventing one back would be a third "
            "placement rule nobody asked for"
        )
        assert "is not detached" in refused(tf, "redock", "keymap#2"), "G: only a floater docks"

        # ── (H) ★ maximise hands back the way home ───────────────────────
        assert_eq(q(tf, "maximized"), "", "H: nothing is maximised")
        assert_eq(q(tf, "restore_to"), "", "H: so there is no way home to read")
        before = json.loads(q(tf, "layout"))
        assert_eq(inv(tf, "act", "decode#1,maximize"), "decode#1 maximize", "H: maximise")
        filled = json.loads(q(tf, "layout"))
        assert_eq(len(filled["tiles"]), 1, "H: one card fills the board")
        assert_eq(filled["tiles"][0]["w"], 12, "H: across every column")
        assert_eq(
            json.loads(q(tf, "restore_to")),
            before,
            "H: ★ and the way home is READABLE before it is taken — the token IS "
            "the previous arrangement, so there is no second copy in the "
            "binding to fall out of date",
        )
        assert "already maximised" in refused(tf, "act", "keymap#2,maximize"), "H: one at a time"
        assert_eq(inv(tf, "restore", None), "decode#1", "H: restore names what it restored")
        assert_eq(json.loads(q(tf, "layout")), before, "H: and the arrangement is back, exactly")
        assert "no card is maximised" in refused(tf, "restore", None), "H: restoring twice"

        # ── (I) ★ named layouts, from the menu a person opens ────────────
        # ★ R1896 — FOUR shipped layouts, not one. R1894 put the behaviour
        # canon's own `builtinPresets()` boards in, so the menu opens with the
        # four this application ships, ordered by name.
        assert_eq(
            q(tf, "presets"),
            "Latency,Overview,Topology focus,Traffic",
            "I: the four layouts this application ships, in name order",
        )
        assert_eq(q(tf, "preset_open"), False, "I: the menu is closed")
        click(tf, at(tf, "shell.subbar.preset"))
        assert_eq(q(tf, "preset_open"), True, "I: pressing the preset chip opens it")
        saved = len(q(tf, "presets").split(","))
        click(tf, at(tf, f"shell.preset.item.{saved}"))
        assert_eq(
            q(tf, "presets"),
            "Latency,Layout 5,Overview,Topology focus,Traffic",
            "I: ★ saved under a derived name, and sorted in among the rest",
        )
        assert_eq(q(tf, "preset"), "Layout 5", "I: which becomes the current layout")
        assert_eq(q(tf, "preset_open"), False, "I: and the menu closed")
        cards_then = q(tf, "cards")
        tf.intervene(f"{EXT}/preset", "Overview")
        assert_eq(
            q(tf, "cards"),
            ",".join(OPENING),
            "I: ★ applying a layout restores BOTH the arrangement and WHICH "
            "CARDS were on it — a preset that restored only the cells would put "
            "the previous board's cards into the new layout's holes",
        )
        assert cards_then != q(tf, "cards"), "I: which really is a different board"
        # ★ R1896 — the refusal NAMES the set now. R1893 moved this rule into
        # `pinion_core::workspace`, whose refusal lists what would have worked;
        # it used to read "is not a saved layout" and leave the caller guessing.
        refusal = refused_write(tf, "preset", "nope")
        assert "Overview" in refusal and "Traffic" in refusal, (
            f"I: a closed set, and the refusal says what is in it: {refusal!r}"
        )

        # ── (J) ★ the derivation decides the PAINT, not the card kind ────
        inv(tf, "set_state", "keymap#2,denied,read scope")
        inv(tf, "set_state", "decode#1,opaque")
        snap = paint(tf)
        denied = find_by_tag(snap, "card.keymap#2.remedy")
        opaque = find_by_tag(snap, "card.decode#1.remedy")
        assert denied is not None and opaque is not None, "J: both cards paint a remedy"
        assert texts_of(denied) == ["Request access"], (
            f"J: ★ the denial paints a CONTROL: {texts_of(denied)}"
        )
        assert texts_of(opaque) == ["nothing can be done"], (
            f"J: ★★ and the encrypted link paints prose with no control beside "
            f"it — two cards of different kinds, one derivation, and neither "
            f"card decided: {texts_of(opaque)}"
        )
        assert find_by_tag(snap, "card.packet#0.tear_off") is not None, "J: offered, so painted"
        # ★★★★★ R1846 — THIS ASSERTION WAS VACUOUS AND ITS STATED REASON WAS
        # FALSE. It read `card.packet#5.tear_off is None` and explained the
        # absence as "not offered, so absent from the scene" — but by section J
        # the preset applied in (I) has restored the OPENING board, so
        # `packet#5` is not on it at all. The tag was missing because the CARD
        # was gone, and `tear_off` is one of the four every card carries, which
        # (C) asserts three sections earlier. Found by deriving the ordinals:
        # once `packet#5` became `packet#{next_id}` the line still passed, and
        # asking why is what showed it had never tested what it said.
        #
        # Repaired to the claim the sentence beside it actually makes: the
        # affordance (C) refuses on the wire is the affordance the scene does
        # not paint, on a card that IS on the board.
        assert find_by_tag(snap, "card.packet#0.float") is None, (
            "J: not offered, so absent from the scene — the wire refusal in (C) "
            "and this are the same set, read two ways"
        )

        # ── (K) ★ the paint and the gesture read ONE geometry ────────────
        # The open debt this guards is a surface whose painter and hit test
        # compute rectangles separately, so a control ends up drawn where it
        # cannot be clicked. Both directions, because each misses what the other
        # catches — and the second is what found R1648 painting every card's
        # contents at twice their intended offset.
        tags = {node.get("tag") for _p, node in walk_nodes(snap) if node.get("tag")}
        probed, named = 0, 0
        for py in range(4, 900, 44):
            for px in range(4, 1440, 52):
                where = inv(tf, "point", f"{px},{py}")
                probed += 1
                if where == "nothing":
                    continue
                named += 1
                assert where in tags, (
                    f"K: ★ ({px},{py}) hit-tests as {where!r}, which the scene "
                    f"never painted — the gesture and the paint are reading two "
                    f"different facts"
                )
        assert probed > 400 and named > 200, f"K: the sweep covers the window: {named}/{probed}"

        words = q(tf, "affordances").split(",") + q(tf, "steppers").split(",")
        # ★ R1664 — WINDOW-absolute rects, for the reason `at` states: a node
        # inside the board's scroll pane carries a scroll-LOCAL rect, so this
        # population was aiming presses at coordinates nothing is painted at
        # (a card control reported y=20, which is the app bar).
        tagged = [
            (tag, {"x": x, "y": y, "w": w, "h": h})
            for tag, (x, y, w, h) in abs_rects_of(snap).items()
        ]
        # ★★★★★ R1733 — a region the screen has DECLARED quiet is not a control,
        # and that is derived rather than listed. R1728's rule: an exception
        # written by name is only as good as whoever last updated it, and this
        # population's exclusions had already grown to three. A palette row's
        # four parts are drawn inside the row and the ROW is what a press
        # answers for, so each of them declares its silence — sixteen regions,
        # none of which had to be named here.
        #
        # ⚠ What this trades, stated rather than hidden: a real control that is
        # WRONGLY declared silent now leaves this population instead of failing
        # here. That is not unwatched — the voice census's own arms are what
        # judge a mis-declaration (`ghost`, `dangling`, `mumbled`), and
        # `r1694_a_locked_seat_is_heard` asserts every one of them is zero. The
        # alternative was a fourth hand-written exclusion, which is the shape
        # R1728 measured as only being as good as whoever last updated it.
        quiet = {
            node["tag"]
            for node in tf.request("scene/voice").result["nodes"]
            if node.get("voice") == "silent"
        }
        controls = [
            (tag, rect)
            for tag, rect in tagged
            # A badge is an address, not a control (R1613), so the account chip
            # and the DETACHED badge are not in this population.
            if tag != "shell.rail.account"
            and not tag.endswith(".badge")
            and tag not in quiet
            # ★ R1694 — the palette's section headings and its two counts are
            # addressable so a reader can walk into a group and hear how many
            # seats are placed and reserved. They are ANNOUNCED readouts, so the
            # derivation above does not reach them and the exclusion is by name.
            and not tag.startswith("shell.palette.section.")
            and tag not in ("shell.palette.placed", "shell.palette.reserved")
            and (
                tag.startswith(
                    ("shell.appbar.", "shell.subbar.", "shell.rail.", "shell.palette.")
                )
                or any(tag.endswith(f".{word}") for word in words)
            )
        ]
        assert len(controls) >= 30, f"K: the shell paints controls to check: {len(controls)}"
        for tag, rect in controls:
            assert_eq(
                inv(tf, "point", centre(rect)),
                tag,
                f"K: ★★ {tag} is painted at {rect} and must be pressable there",
            )
        # ★ And the deliberate asymmetry: a remedy nobody can act on is PROSE.
        # It carries a tag, because a tag is an address and not a claim of
        # clickability, and pressing where it is drawn selects the card.
        assert find_by_tag(snap, "card.decode#1.remedy") is not None, (
            "K: the board has a non-actionable remedy on it"
        )
        # ★ R1664 — through `at`, not `find_by_tag(...)["rect"]`. This line was
        # the third site reading a node's own rect as a window position, and it
        # is the one the other two repairs left behind: the board became a scroll
        # pane in R1662, so the remedy's own rect is scroll-LOCAL and this press
        # landed on the app bar. Naming the tag instead of carrying a rect is
        # what makes a fourth site impossible rather than merely unlikely.
        assert_eq(
            inv(tf, "point", at(tf, "card.decode#1.remedy")),
            "card.decode#1",
            "K: ★ an encrypted link's remedy is drawn but is not a control",
        )
        assert "is outside the" in refused(tf, "point", "9999,10"), "K: off-window"

        # ★★ And TEXT is placed where it was asked for, not flowed. A `TextNode`
        # carries a rect, but without a layout style the parent lays it out IN
        # FLOW — so a set of labels written at deliberate coordinates stacks
        # down the left edge instead. This shell shipped exactly that and
        # nothing in this file noticed, because every assertion above reads
        # TAGS and TEXTS and the rects of tagged CONTAINERS, and a text run
        # carries no tag. A counterfactual found it; this is what closes it.
        card = find_by_tag(snap, "card.packet#0")
        assert card is not None, "K: the card is painted"
        first = spec["stream_rows"][0]
        SPEC_FIRST_ROW = (first["time"], first["type"], first["name"], first["len"])
        origin = card["rect"]
        # ★ A LIST, not a dict keyed by content. R1668: this was a dict, and the
        # message stream repeats a type word down its rows, so four of them
        # collapsed into one and the card looked as though it had lost cells.
        # A demo that indexes paint by its text cannot see a table.
        runs = [
            (str(node.get("content")), node["rect"])
            for _p, node in walk_nodes(card)
            if isinstance(node.get("content"), str) and isinstance(node.get("rect"), dict)
        ]
        title = next((r for text, r in runs if text == "Message Stream"), None)
        assert title is not None, f"K: the card's text: {[t for t, _ in runs]}"
        assert title["x"] - origin["x"] > 30, (
            f"K: ★★ the title sits BESIDE the drag grip, not under it — flowed "
            f"text would start at the container's origin: {title}"
        )
        # And the body is a TABLE: the four cells of one row share a baseline
        # and sit side by side. Flowed text stacks, so this is the property
        # that separates a table from a list of everything the card holds.
        row = find_by_tag(card, "card.packet#0.row.0")
        assert row is not None, "K: the first stream row is painted"
        cells = [
            (str(node.get("content")), node["rect"])
            for _p, node in walk_nodes(row)
            if isinstance(node.get("content"), str) and isinstance(node.get("rect"), dict)
        ]
        assert_eq([text for text, _ in cells], list(SPEC_FIRST_ROW), "K: the row's four cells")
        assert len({r["y"] for _t, r in cells}) == 1, f"K: ★ one baseline: {cells}"
        xs = [r["x"] for _t, r in cells]
        assert xs == sorted(xs) and len(set(xs)) == len(xs), (
            f"K: ★★ and four distinct columns left to right, which flowed text "
            f"cannot produce: {cells}"
        )

        # ── (L) the pointer and the keyboard are one set of handlers ─────
        assert_eq(inv(tf, "point", at(tf, "shell.appbar.capture")), "shell.appbar.capture", "L: aim")
        was = q(tf, "capturing")
        inv(tf, "send", "PointerDown")
        inv(tf, "send", "PointerUp")
        assert q(tf, "capturing") != was, "L: a click toggled capture"
        # ★ A control fires on RELEASE over the same target, so a press that
        # slides off is abandoned — and a press that is INTERRUPTED is not a
        # release either.
        cards_before = q(tf, "cards")
        target = at(tf, "card.keymap#2.close")
        inv(tf, "point", target)
        inv(tf, "send", "PointerDown")
        inv(tf, "point", "760,760")
        inv(tf, "send", "PointerUp")
        assert_eq(q(tf, "cards"), cards_before, "L: ★ released elsewhere, so nothing closed")
        inv(tf, "point", target)
        inv(tf, "send", "PointerDown")
        inv(tf, "send", "PointerCancel")
        inv(tf, "send", "PointerUp")
        assert_eq(q(tf, "cards"), cards_before, "L: ★ a cancelled press performs nothing")
        click(tf, target)
        assert q(tf, "cards") != cards_before, "L: and released on it, so it closed"
        assert "is not a pointer event" in refused(tf, "send", "PointerSideways"), "L: closed set"

        first = q(tf, "cards").split(",")[0]
        click(tf, at(tf, f"card.{first}"))
        assert_eq(q(tf, "selected"), first, "L: pressing a card selects it")
        assert_eq(inv(tf, "key", "ArrowRight"), True, "L: the arrow is claimed")
        assert_eq(inv(tf, "key", "F13"), False, "L: an unclaimed chord stays unclaimed")
        moved_to = q(tf, "selected")
        placed_grid = json.loads(q(tf, "layout"))
        assert_eq(inv(tf, "key", "Shift+ArrowDown"), True, "L: nudge the selected card")
        assert_eq(q(tf, "selected"), moved_to, "L: which does NOT move the selection")
        assert row_of(json.loads(q(tf, "layout")), moved_to) > row_of(placed_grid, moved_to), (
            "L: ★ Shift+arrow moved the card rather than the selection"
        )

        # ★ The search box is typed into behind a mode that says it is on —
        # otherwise the letter shortcuts would eat the text.
        assert_eq(inv(tf, "key", "c"), True, "L: `c` is a shortcut outside the box")
        tf.intervene(f"{EXT}/search", "")
        assert_eq(inv(tf, "key", "/"), True, "L: `/` opens the search")
        for letter in "syn":
            assert_eq(inv(tf, "key", letter), True, f"L: {letter!r} is text now")
        assert_eq(q(tf, "search"), "syn", "L: which lands in the search box")
        assert_eq(inv(tf, "key", "Backspace"), True, "L: backspace deletes")
        assert_eq(q(tf, "search"), "sy", "L: one character")
        assert_eq(inv(tf, "key", "Escape"), True, "L: and Escape leaves the box")
        assert_eq(inv(tf, "key", "c"), True, "L: after which `c` is a shortcut again")

        keymap = q(tf, "keymap")
        for chord in ["Arrow=", "Shift+Arrow=", "Enter=", "Escape=", "/=", "e=", "o="]:
            assert chord in keymap, f"L: {chord} is published: {keymap}"

        # ── (M) the transport is derived, not a fourth clock state ───────
        tf.intervene(f"{EXT}/capturing", True)
        assert_eq(q(tf, "transport"), "live", "M: capture on and nothing replaying")
        assert_eq(q(tf, "playhead"), 0, "M: the playhead is parked")
        assert_eq(inv(tf, "seek", "400"), 400, "M: scrub into the replay window")
        assert_eq(q(tf, "playhead"), 400, "M: the playhead moved")
        assert_eq(
            q(tf, "transport"),
            "paused",
            "M: ★ and 'live' went away without anyone setting it — it is the "
            "absence of a replay while capture is on, derived from the existing "
            "TransportClock rather than being a fourth state to keep in step",
        )
        assert "0..=1000" in refused(tf, "seek", "1400"), "M: outside the window"

        # ── (M2) ★★★★★ R1797 — the latency card, and the ONE record ───────
        #
        # The reference draws this card as eight buckets under three tiles and
        # publishes both sets of numbers independently — and they disagree: its
        # own bar counts put the 95th percentile in `16-32` while its tile says
        # 11.4 ms, which is in `8-16`. A mockup cannot notice that. This card
        # derives the bars, the tiles and the emphasis from ONE capture record,
        # so it cannot say two things about one distribution, and the checks
        # below are what makes that a claim rather than a hope.
        snap = paint(tf)
        for n, key in enumerate(("p50", "p95", "max")):
            tile = find_by_tag(snap, f"card.latency#4.stat.{n}")
            assert tile is not None, f"M2: the {key} tile is painted"
            words = texts_of(tile)
            assert key in words, f"M2: the {key} tile names its landmark: {words}"
            assert LATENCY_TILES[key] in words, (
                f"M2: ★ and DERIVES the reference's published figure "
                f"{LATENCY_TILES[key]!r} rather than printing it: {words}"
            )
        assert find_by_tag(snap, "card.latency#4.bins") is not None, "M2: the bars are drawn"
        caption = find_by_tag(snap, "card.latency#4.caption")
        assert caption is not None, "M2: and the caption under them"
        assert "p95" in " ".join(texts_of(caption)), (
            f"M2: ★★ which says WHY a bar is emphasised. The reference's says "
            f"'tail in amber' and its tail is a hard-coded index, so its caption "
            f"cannot be checked against anything: {texts_of(caption)}"
        )
        # ★★★ The bucket ladder itself, read off the painted bars. Two of the
        # eight are UNBOUNDED — `<1` and `>64` — which the floor's numeric axis
        # measurably cannot express: handed +inf it refuses and keeps its old
        # maximum. Here the open end is what stops the 72 ms reply being dropped
        # while the `max` tile still reported it.
        # `walk_nodes` yields `(path, node)` — the path is for failure messages.
        drawn = [
            t
            for _, node in walk_nodes(snap)
            if (node.get("tag") or "").startswith("card.latency#4.dist.")
            for t in texts_of(node)
        ]
        for bucket in LATENCY_BUCKETS:
            assert bucket in drawn, f"M2: the {bucket!r} bucket is on the chart: {drawn}"

        # ★★★★★ And the DERIVATION on the wire, which is where this card goes
        # past the floor: the floor's bar surface has no name for a rule or a
        # basis at all, measured this round by enumerating it at runtime. A
        # reader looking at a surprising distribution has two hypotheses — the
        # data is like that, and the binning did that — and these fields are
        # what tell them apart without a pixel.
        lat = q(tf, "spec")["latency"]
        assert_eq(lat["binned"], True, "M2: the record binned")
        assert_eq(lat["rule"], "stated", "M2: ★ the wire says WHICH rule chose the bins")
        assert_eq(lat["ends"], "open", "M2: and that the outer bins are unbounded")
        assert_eq(
            [b["label"] for b in lat["buckets"]],
            LATENCY_BUCKETS,
            "M2: the published ladder is the reference's",
        )
        assert_eq(
            sum(b["count"] for b in lat["buckets"]),
            lat["basis"]["n"],
            "M2: ★★ every sample the basis counted is in a bucket — an open end "
            "is a BIN, not a discard",
        )
        assert_eq(lat["outside"], {"below": 0, "above": 0}, "M2: so nothing fell out")
        assert_eq(
            lat["basis"]["quantile_method"],
            "linear",
            "M2: ★ and the basis says WHICH quantile definition made its IQR — "
            "three private copies in one crate disagreed about that until R1797",
        )
        assert_eq(
            [t["value"] for t in lat["tiles"]],
            [LATENCY_TILES[k] for k in ("p50", "p95", "max")],
            "M2: the published tiles are the painted ones",
        )
        # ★★ The consistency the reference's own card fails, done from OUTSIDE:
        # walk the published counts to the 95th percentile and check the bucket
        # it lands in contains the published cut.
        cut = lat["tail_cut"]
        seen, landed = 0, lat["buckets"][-1]
        for bucket in lat["buckets"]:
            seen += bucket["count"]
            if seen >= 0.95 * lat["basis"]["n"]:
                landed = bucket
                break
        assert (landed["lo"] is None or cut >= landed["lo"]) and (
            landed["hi"] is None or cut <= landed["hi"]
        ), (
            f"M2: ★★★★★ the p95 tile says {cut} and the published bars put the "
            f"95th percentile in {landed['label']!r} — the reference publishes "
            f"exactly this contradiction and cannot notice it"
        )
        assert_eq(
            [b["label"] for b in lat["buckets"] if b["tail"]],
            ["16-32", "32-64", ">64"],
            "M2: ★ and the emphasised bars are the ones at or above that cut, "
            "derived rather than an index",
        )

        # ── (N) ★★★★★ and a REAL press, through the §5.35 router ─────────
        #
        # Everything above — including section K, which is the one about being
        # pressable — drives `invoke("point")` + `invoke("send")`, the shell's own
        # oracle. R1649.1 measured what that hides: this shell was dead to a
        # mouse at every point in the window while its 118 assertions passed,
        # because the router has to *find* the widget from a bare coordinate and
        # the oracle is handed the answer. R1663 then shipped a sibling screen
        # with the same defect, which says the lesson did not survive as prose.
        #
        # `scene/click {at}` is the wire verb that goes through the router. Two
        # targets, in different panes, so a repair that happens to fix one
        # region does not read as coverage of the screen.
        # ★ R1695 — aimed at `settings` rather than `stream`, because `stream`
        # is now a destination this application cannot take you to and a press
        # on it correctly moves nothing. That the swap was FORCED is the point:
        # before this round every seat moved the string and none of them
        # arrived anywhere, so this assertion was true of a rail that navigated
        # nothing at all.
        assert_router_press_moves(
            tf, "shell.rail.settings", lambda: q(tf, "nav"), "N: a rail seat"
        )
        tf.intervene(f"{EXT}/nav", "dashboard")
        tf.tick(16)
        assert_router_press_moves(
            tf, "shell.subbar.edit", lambda: q(tf, "editing"), "N: the layout-edit toggle"
        )
        # ★ The negative control: the same verb, a point that is decoration,
        # nothing moves. Without it "the screen moved" could be an artefact of
        # pressing at all rather than a fact about where.
        before = (q(tf, "nav"), q(tf, "editing"))
        tf.request("scene/click", {"button": "left", "at": {"x": 2, "y": 2}})
        tf.tick(16)
        assert_eq(
            (q(tf, "nav"), q(tf, "editing")),
            before,
            "N: ★ a press in the app bar's corner moves nothing",
        )


run_demo("R1649 the analyzer shell matches the reference", body)
