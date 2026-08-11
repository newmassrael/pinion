#!/usr/bin/env python3
"""R1648 §5.21 §5.51 §2 #7 — the analysis-tool dashboard shell, assembled, and
what a card says about what it is showing.

`tools/analyzer_census.py` gives every capability of this tool class one of five
verdicts, and the biggest bin is `app` — *the substrate is here, the domain
logic is the application's*. Twenty-six rows said that and **nobody had ever
assembled one**. A `have` is proven by a test that drives the capability through
the public API (R1602); an `app` is a claim about COMPOSITION, and the only
thing that proves a composition is a composite.

`hello-analyzer-shell` is that composite: one binary with an app bar, a
navigation rail, a twelve-card board on `TileGrid`, named layout presets and a
transport. This script is what makes the claim checkable, and the census's
`app` rows now name it in `assembled_by`.

Three things it asserts that a page of separate examples cannot:

* **A header is a set, and the set is enforced.** `CardChrome` is four values
  and a card that does not offer `tear_off` REFUSES it on the wire, naming what
  it does offer. A shell that only hides the button leaves an agent able to do
  what the screen says is impossible.
* **A body state carries its own remedy, derived.** The capability list asks for
  loading, empty, error, no-permission and encrypted states. The two that look
  identical on screen — a permission denial and an encrypted link — are
  OPPOSITE in what a person can do, and here `denied` is actionable and
  `opaque` is not, because `CardState::remedy` decided it and not the card kind.
  Measured on the toolkit at 6.11: it has no content-state concept at all.
* **Maximise hands back the way home.** `TileGrid::maximize` returns a
  `Maximized` token that IS the previous arrangement — there is no second copy
  anywhere in the binding — so `restore_to` can be read before restoring, and a
  second maximise cannot clobber the first.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1648_the_analyzer_shell_is_assembled.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_declared_channels_are_true,
    assert_eq,
    find_by_tag,
    run_demo,
    texts_of,
    walk_nodes,
)

EXT = "/external"

#: The twelve widget kinds the capability list names, in board order.
CARDS = [
    "stream",
    "inspector",
    "topology",
    "throughput",
    "share",
    "latency",
    "loss",
    "kpi",
    "alarms",
    "search",
    "console",
    "report",
]


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


#: The mid-header y of each board row, where a card's affordance strip sits.
#: Derived from the shell's own constants (app bar 40, row 96, pad 4, header 20)
#: rather than guessed, and the sweep in (K) is what proves the derivation
#: agrees with what the shell actually paints.
HEADER_YS = [40 + row * 96 + 4 + 10 for row in range(5)]


def scan_for(tf: RpcSubprocess, suffix: str) -> list[str]:
    """Every `"<x>,<y>"` whose hit name ends with `suffix`, one per control.

    Asks the shell where its controls are instead of recomputing its layout —
    the point of the section this serves is that there is ONE geometry, and a
    demo carrying a second copy of it would be unable to notice a drift.
    """
    found: dict[str, str] = {}
    for py in HEADER_YS:
        for px in range(56, 1040, 13):
            where = inv(tf, "point", f"{px},{py}")
            if where.endswith(suffix) and where not in found:
                found[where] = f"{px},{py}"
    return list(found.values())


def find_point(tf: RpcSubprocess, name: str) -> str:
    """A `"<x>,<y>"` the shell hit-tests as exactly `name`.

    Loud when there is none: a demo that quietly skipped this would be testing
    the gesture on whatever happened to be under a hard-coded coordinate, which
    changes every time an earlier section closes a card.
    """
    for py in range(44, 540, 11):
        for px in range(56, 1040, 21):
            if inv(tf, "point", f"{px},{py}") == name:
                return f"{px},{py}"
    raise AssertionError(f"nothing on the window hit-tests as {name!r}")


def body() -> None:
    with RpcSubprocess("hello-analyzer-shell", boot_grace=1.5) as tf:
        counted = assert_declared_channels_are_true(tf)
        assert counted["read"] >= 18, f"the walk reaches the surface: {counted}"

        # ── (A) the shell is assembled ───────────────────────────────────
        assert_eq(q(tf, "cards"), ",".join(CARDS), "A: twelve cards, in board order")
        assert_eq(q(tf, "card_count"), 12, "A: and it says how many")
        assert_eq(
            q(tf, "rail"),
            "capture:3,topology:1,metrics:5,operate:3",
            "A: the navigation rail, with each section's live card count — the "
            "rail is derived from the board rather than being a second list",
        )
        # The app bar's four slots, all writable, all on the wire.
        assert_eq(q(tf, "source"), "live-capture", "A: the app bar opens on a source")
        assert_eq(q(tf, "capturing"), True, "A: capture is on")
        assert_eq(q(tf, "search"), "", "A: global search is empty")
        tf.intervene(f"{EXT}/source", "lab-replay")
        tf.intervene(f"{EXT}/search", "handshake")
        tf.intervene(f"{EXT}/capturing", False)
        tf.intervene(f"{EXT}/theme", "dark")
        assert_eq(q(tf, "source"), "lab-replay", "A: source chosen")
        assert_eq(q(tf, "search"), "handshake", "A: search typed")
        assert_eq(q(tf, "capturing"), False, "A: capture toggled")
        assert_eq(q(tf, "theme"), "dark", "A: theme toggled")
        assert "is not a source" in refused_write(tf, "source", "nope"), (
            "A: and a source outside the offered set is refused BY NAME"
        )
        assert_eq(q(tf, "source"), "lab-replay", "A: the refusal changed nothing")
        # Put the bar back so the transport section below starts from live.
        tf.intervene(f"{EXT}/capturing", True)

        # ── (B) ★ a header is a set, and the set is enforced ─────────────
        assert_eq(
            q(tf, "affordances"),
            "settings,tear_off,maximize,close",
            "B: the four affordances, published in layout order",
        )
        assert_eq(
            inv(tf, "chrome", "stream"),
            "settings,tear_off,maximize,close",
            "B: ★ ONE card offering tear-off AND maximise — the toolkit splits "
            "those across two class hierarchies that cannot be combined, so a "
            "card with both is not expressible there at all",
        )
        assert_eq(
            inv(tf, "chrome", "report"),
            "settings,close",
            "B: and a card that offers less says so",
        )
        why = refused(tf, "act", "report,tear_off")
        assert "does not offer tear_off" in why, (
            f"B: ★ an affordance the card does not offer is refused BY NAME, on "
            f"the wire, not merely hidden from the painter: {why}"
        )
        assert "settings,close" in why, "B: and the refusal says what it DOES offer"
        assert "is not an affordance" in refused(tf, "act", "report,float"), (
            "B: a word outside the vocabulary is a different refusal"
        )
        assert "is not <card>,<affordance>" in refused(tf, "act", "report"), (
            "B: and a malformed argument is a third"
        )
        assert_eq(inv(tf, "act", "report,settings"), "report settings", "B: what it offers works")

        # ── (C) ★ the body state, and its DERIVED remedy ─────────────────
        assert_eq(
            q(tf, "states"),
            "ready,loading,empty,failed,denied,opaque",
            "C: six states — the capability list's five reasons there is no "
            "content, plus content",
        )
        assert_eq(q(tf, "remedies"), "wait,retry,widen,authorize,nothing", "C: five remedies")
        seeded = {card: inv(tf, "state", card) for card in CARDS}
        assert set(seeded.values()) == {
            "ready",
            "loading",
            "empty",
            "failed",
            "denied",
            "opaque",
        }, (
            f"C: ★ every arm of the vocabulary is on the board at once — a shell "
            f"whose cards are all ready never exercises the half of this design "
            f"that matters: {seeded}"
        )
        assert_eq(inv(tf, "remedy", "throughput"), "wait", "C: loading -> wait")
        assert_eq(inv(tf, "remedy", "loss"), "widen", "C: empty -> widen the filter")
        assert_eq(inv(tf, "remedy", "latency"), "retry", "C: failed -> retry")
        assert_eq(inv(tf, "remedy", "kpi"), "none", "C: a ready card has NO remedy")
        assert_eq(
            (inv(tf, "remedy", "console"), inv(tf, "actionable", "console")),
            ("authorize", "yes"),
            "C: ★ a denial is actionable — somebody holds the right",
        )
        assert_eq(
            (inv(tf, "remedy", "report"), inv(tf, "actionable", "report")),
            ("nothing", "no"),
            "C: ★★ and an encrypted link is NOT, though both render as 'no "
            "content'. Collapsing them into one `error` arm is what makes a "
            "shell offer 'request access' on a link no permission can open",
        )
        assert_eq(
            inv(tf, "detail", "latency"),
            "collector unreachable",
            "C: the two arms that carry a particular reason say it",
        )
        assert_eq(inv(tf, "detail", "console"), "operator role", "C: which right is missing")
        assert_eq(
            inv(tf, "detail", "report"),
            "",
            "C: and the arms whose explanation is the same every time carry none",
        )

        # ── (D) the detail is required by exactly the arms that carry one ─
        assert "carries a reason" in refused(tf, "set_state", "kpi,failed"), (
            "D: a failure with no reason is a failure whose reason was lost"
        )
        assert "carries no reason" in refused(tf, "set_state", "kpi,empty,because"), (
            "D: and a reason on an arm nothing reads it from is refused too"
        )
        assert_eq(
            inv(tf, "set_state", "kpi,denied,audit scope"),
            "kpi denied authorize",
            "D: a state change reports the state AND the remedy it derives",
        )
        assert_eq(inv(tf, "actionable", "kpi"), "yes", "D: which the card now publishes")
        assert_eq(inv(tf, "set_state", "kpi,ready"), "kpi ready none", "D: and back")
        assert "is not a card state" in refused(tf, "set_state", "kpi,broken"), "D: closed set"
        assert "no card" in refused(tf, "state", "nosuch"), "D: an unknown card is named"

        # ── (E) ★ maximise hands back the way home ───────────────────────
        assert_eq(q(tf, "maximized"), "", "E: nothing is maximised")
        assert_eq(q(tf, "restore_to"), "", "E: so there is no way home to read")
        before = json.loads(q(tf, "layout"))
        assert_eq(len(before["tiles"]), 12, "E: twelve tiles on the board")
        assert_eq(inv(tf, "act", "topology,maximize"), "topology maximize", "E: maximise it")
        assert_eq(q(tf, "maximized"), "topology", "E: the board says which card")
        filled = json.loads(q(tf, "layout"))
        assert_eq(len(filled["tiles"]), 1, "E: one card fills the board")
        assert_eq(filled["tiles"][0]["w"], 12, "E: across every column")
        assert_eq(
            json.loads(q(tf, "restore_to")),
            before,
            "E: ★ and the way home is READABLE before it is taken — the token IS "
            "the previous arrangement, so there is no second copy in the "
            "binding to fall out of date",
        )
        assert "already maximised" in refused(tf, "act", "stream,maximize"), (
            "E: a second maximise is refused rather than clobbering the first"
        )
        assert_eq(inv(tf, "restore", None), "topology", "E: restore names what it restored")
        assert_eq(json.loads(q(tf, "layout")), before, "E: and the arrangement is back, exactly")
        assert_eq(q(tf, "maximized"), "", "E: with nothing maximised")
        assert "no card is maximised" in refused(tf, "restore", None), "E: restoring twice is refused"

        # ── (F) named layout presets ─────────────────────────────────────
        assert_eq(q(tf, "presets"), "default", "F: one saved layout to start")
        assert_eq(inv(tf, "act", "share,maximize"), "share maximize", "F: rearrange")
        assert_eq(inv(tf, "save_preset", "focus"), "default,focus", "F: save it under a name")
        assert_eq(q(tf, "preset"), "focus", "F: which becomes the current layout")
        tf.intervene(f"{EXT}/preset", "default")
        assert_eq(json.loads(q(tf, "layout")), before, "F: and switching back restores twelve cards")
        assert_eq(
            q(tf, "maximized"),
            "",
            "F: applying a preset drops the maximise — a way home to an "
            "arrangement nobody is on any more is worse than none",
        )
        assert "is not a saved layout" in refused_write(tf, "preset", "nope"), "F: unknown preset"

        # ── (G) the transport is derived, not a fourth clock state ───────
        assert_eq(q(tf, "transport"), "live", "G: capture on and nothing replaying")
        assert_eq(q(tf, "playhead"), 0, "G: the playhead is parked")
        assert_eq(inv(tf, "seek", "400"), 400, "G: scrub into the replay window")
        assert_eq(q(tf, "playhead"), 400, "G: the playhead moved")
        assert_eq(
            q(tf, "transport"),
            "paused",
            "G: ★ and 'live' went away without anyone setting it — it is the "
            "absence of a replay while capture is on, derived from the existing "
            "TransportClock rather than being a fourth state to keep in step",
        )
        assert "0..=1000" in refused(tf, "seek", "1400"), "G: a playhead outside the window"

        # ── (H) ★ tear-off drives the dock lifecycle, and keeps its place ─
        assert_eq(q(tf, "floating"), "", "H: nothing is torn off")
        assert_eq(inv(tf, "act", "stream,tear_off"), "stream tear_off", "H: tear one off")
        assert_eq(
            q(tf, "floating"),
            "stream",
            "H: which the DOCK STATECHART says, not a flag this binding keeps — "
            "the tear-off sends `escaped` to the same DockPanelPolicy the dock "
            "surface uses, so there is one float model rather than two",
        )
        placed = json.loads(q(tf, "layout"))
        assert any(t["id"] == "stream" for t in placed["tiles"]), (
            "H: ★ and the card KEEPS ITS PLACE on the board. Reflowing on "
            "tear-off and again on dock-back is how a dashboard loses a layout "
            "to a gesture that was meant to be temporary"
        )
        assert_eq(q(tf, "card_count"), 12, "H: it is displayed elsewhere, not gone")
        assert "is not torn off" in refused(tf, "dock_back", "kpi"), "H: only a floater docks back"
        assert_eq(inv(tf, "dock_back", "stream"), "", "H: and it comes home")
        assert_eq(q(tf, "floating"), "", "H: with nothing floating")

        # ── (I) close removes the card, and the rail count moves ─────────
        assert_eq(inv(tf, "act", "alarms,close"), "alarms close", "I: close one")
        assert_eq(q(tf, "card_count"), 11, "I: eleven cards left")
        assert "alarms" not in q(tf, "cards"), "I: and it is gone from the board"
        assert_eq(
            q(tf, "rail"),
            "capture:3,topology:1,metrics:5,operate:2",
            "I: ★ the rail's operate count fell — the navigation is derived from "
            "the board, so it cannot claim a section still has a card it lost",
        )
        assert "no card" in refused(tf, "act", "alarms,settings"), "I: acting on it now is refused"

        # ── (J) ★ the derivation decides the PAINT, not the card kind ────
        # The wire assertions above prove `remedy` answers. This proves the
        # shell OBEYS it: an assertion about what is drawn has to read the
        # drawing (R1624), and "only an actionable remedy gets a control" is a
        # claim about pixels that no `remedy` read can make.
        snap = tf.snapshot(source="paint", viewport=(1040, 566))
        console = find_by_tag(snap, "card.console.remedy")
        report = find_by_tag(snap, "card.report.remedy")
        waiting = find_by_tag(snap, "card.throughput.remedy")
        assert console is not None and report is not None, "J: both cards paint a remedy"
        assert texts_of(console) == ["[ Request access ]"], (
            f"J: ★ the denial paints a CONTROL: {texts_of(console)}"
        )
        assert texts_of(report) == ["nothing can be done"], (
            f"J: ★★ and the encrypted link paints the same sentence with NO "
            f"control beside it — two cards of different kinds, one derivation, "
            f"and neither card decided: {texts_of(report)}"
        )
        assert texts_of(waiting) == ["waiting"], (
            f"J: a loading card is the card's own job, so it gets no control "
            f"either — for the opposite reason: {texts_of(waiting)}"
        )
        # And the header set reaches the paint: a card that does not offer
        # tear-off has no tear-off node at all, rather than a disabled one.
        assert find_by_tag(snap, "card.stream.tear_off") is not None, "J: offered, so painted"
        assert find_by_tag(snap, "card.report.tear_off") is None, (
            "J: not offered, so absent from the scene — the wire refusal in (B) "
            "and this are the same set, read two ways"
        )
        # The KPI card is a box, a label and a chart primitive, which is exactly
        # what that capability row claims an application assembles.
        assert find_by_tag(snap, "card.kpi.sparkline") is not None, (
            "J: the KPI stat tile carries a real pinion-chart sparkline"
        )

        # ── (K) ★ the shell is operable by hand, and by the same verbs ───
        # The user requirement this section exists for: the assembly has to be
        # something a person drives with a mouse and a keyboard, not a wire-only
        # surface with a picture attached. Every call below goes through the
        # handler a real press and a real chord reach — `point` moves the same
        # cursor `pointer_move` does, `send` carries the router's own symbolic
        # events, and `key` takes the chord the platform spells.
        tags = {
            node.get("tag")
            for _path, node in walk_nodes(tf.snapshot(source="paint", viewport=(1040, 566)))
            if node.get("tag")
        }
        # ★ Everything the hit test can NAME is something the scene DREW. The
        # open debt this guards is a surface whose painter and hit test compute
        # rectangles separately, so a control ends up drawn where it cannot be
        # clicked; here both read one geometry, and this samples the window to
        # say so rather than trusting the claim.
        probed, named = 0, 0
        for py in range(4, 566, 46):
            for px in range(4, 1040, 52):
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
        assert probed > 200 and named > 100, (
            f"K: the sweep has to actually cover the window: {named}/{probed}"
        )
        # ★★ And the CONVERSE, which is the direction that matters: every
        # control the scene painted must be hittable AT THE CENTRE OF THE
        # RECTANGLE IT WAS PAINTED IN. The sweep above only says the gesture
        # invents no names; this says the paint has no dead controls, and it is
        # what a counterfactual proved was missing — a hit test computing its
        # own narrower slots passed every other assertion in this file while
        # leaving a third of each affordance unclickable. It also caught the
        # real defect underneath: children of an absolutely-positioned
        # container are placed RELATIVE to it, so the first draft painted every
        # card's contents at twice their intended offset and nothing said so.
        words = q(tf, "affordances").split(",")
        tagged = [
            (node["tag"], node["rect"])
            for _path, node in walk_nodes(snap)
            if isinstance(node.get("rect"), dict) and isinstance(node.get("tag"), str)
        ]
        centre = lambda r: f"{r['x'] + r['w'] // 2},{r['y'] + r['h'] // 2}"
        controls = [
            (tag, rect)
            for tag, rect in tagged
            if tag.startswith(("shell.appbar.", "shell.rail."))
            or any(tag.endswith(f".{word}") for word in words)
            # A remedy is a control only when it is ACTIONABLE — see below.
            or (
                tag.endswith(".remedy")
                and inv(tf, "actionable", tag.split(".")[1]) == "yes"
            )
        ]
        assert len(controls) >= 20, f"K: the board paints controls to check: {len(controls)}"
        for tag, rect in controls:
            assert_eq(
                inv(tf, "point", centre(rect)),
                tag,
                f"K: ★★ {tag} is painted at {rect} and must be pressable there",
            )
        # ★ And the deliberate asymmetry, asserted rather than assumed: a
        # remedy nobody can act on is PROSE. It carries a tag, because a tag is
        # an address and not a claim of clickability (R1613), and pressing where
        # it is drawn selects the card — there is no control there to press.
        inert = [
            (tag, rect)
            for tag, rect in tagged
            if tag.endswith(".remedy") and inv(tf, "actionable", tag.split(".")[1]) == "no"
        ]
        assert inert, "K: the board has a non-actionable remedy on it to check"
        for tag, rect in inert:
            card_id = tag.split(".")[1]
            assert_eq(
                inv(tf, "point", centre(rect)),
                f"card.{card_id}",
                f"K: ★ {tag} is drawn but is not a control — {inv(tf, 'remedy', card_id)} "
                f"is not something a person can do",
            )
        assert "is outside the" in refused(tf, "point", "9999,10"), "K: off-window"

        # The app bar's chips are pressable, and each one moves its own slot.
        chip = find_by_tag(snap, "shell.appbar.capture")
        assert chip is not None, "K: the capture chip is painted"
        was = q(tf, "capturing")
        assert_eq(inv(tf, "point", "330,20"), "shell.appbar.capture", "K: aim at it")
        inv(tf, "send", "PointerDown")
        inv(tf, "send", "PointerUp")
        assert q(tf, "capturing") != was, "K: a click toggled capture"

        # Where the close affordances are, found by ASKING the shell rather than
        # by re-deriving its geometry here. A demo that computed the slot
        # rectangle itself would be the second copy this whole section exists to
        # rule out — and the scan is loud when it finds none, because a guarded
        # "if we happened to land on one" assertion is one that can silently
        # stop running (which is how two of this round's counterfactuals passed
        # on the first attempt).
        closes = scan_for(tf, ".close")
        assert len(closes) >= 2, f"K: the board has close buttons to press: {closes}"
        first_close, second_close = closes[0], closes[1]

        # ★ A control fires on RELEASE over the same target, so a press that
        # slides off is abandoned — the behaviour every desktop toolkit has and
        # the reason a press-to-fire button feels wrong.
        cards_before = q(tf, "cards")
        assert inv(tf, "point", first_close).endswith(".close"), "K: aim at it"
        inv(tf, "send", "PointerDown")
        inv(tf, "point", "600,300")  # slide off it
        inv(tf, "send", "PointerUp")
        assert_eq(q(tf, "cards"), cards_before, "K: ★ released elsewhere, so nothing closed")

        # A press that is INTERRUPTED is not a release either: the latch is
        # dropped without performing it, which is the difference between letting
        # go and having the window taken away.
        inv(tf, "point", first_close)
        inv(tf, "send", "PointerDown")
        inv(tf, "send", "PointerCancel")
        inv(tf, "send", "PointerUp")
        assert_eq(q(tf, "cards"), cards_before, "K: ★ a cancelled press performs nothing")

        # And released ON it, it closes — so the two refusals above are about
        # the gesture and not about a button that never worked.
        inv(tf, "point", first_close)
        inv(tf, "send", "PointerDown")
        inv(tf, "send", "PointerUp")
        assert q(tf, "cards") != cards_before, "K: released on it, so it closed"
        assert "is not a pointer event" in refused(tf, "send", "PointerSideways"), (
            "K: the pointer vocabulary is closed"
        )
        assert second_close != first_close, "K: the scan found distinct slots"

        # Selection: a press selects, the arrows move the selection, and the
        # rail follows — one current card for the pointer and the keyboard
        # rather than two that drift apart.
        target = q(tf, "cards").split(",")[0]
        inv(tf, "point", find_point(tf, f"card.{target}"))
        inv(tf, "send", "PointerDown")
        inv(tf, "send", "PointerUp")
        assert_eq(q(tf, "selected"), target, "K: pressing a card selects it")
        assert_eq(
            q(tf, "rail_focus"),
            inv(tf, "section", target),
            "K: and the rail follows the selection — one current card for the "
            "pointer and the keyboard rather than two that drift apart",
        )
        assert_eq(inv(tf, "key", "ArrowRight"), True, "K: the arrow is claimed")
        moved_to = q(tf, "selected")
        assert moved_to != target, f"K: and it moves the selection: {moved_to}"
        assert_eq(inv(tf, "key", "F13"), False, "K: an unclaimed chord stays unclaimed")

        # Shift+arrow moves the CARD, which is the board edit a person makes
        # without a mouse.
        placed = json.loads(q(tf, "layout"))
        row_of = lambda grid, cid: next(t["row"] for t in grid["tiles"] if t["id"] == cid)
        assert_eq(inv(tf, "key", "Shift+ArrowDown"), True, "K: nudge the selected card")
        assert_eq(q(tf, "selected"), moved_to, "K: which does NOT move the selection")
        moved = json.loads(q(tf, "layout"))
        assert row_of(moved, moved_to) > row_of(placed, moved_to), (
            "K: ★ Shift+arrow moved the card rather than the selection"
        )

        # Enter maximises the selection and Escape restores it — the same two
        # verbs section (E) drove on the wire, reached by a keyboard.
        assert_eq(inv(tf, "key", "Enter"), True, "K: Enter maximises")
        assert_eq(q(tf, "maximized"), moved_to, "K: the selected card fills the board")
        assert_eq(inv(tf, "key", "Escape"), True, "K: Escape restores")
        assert_eq(q(tf, "maximized"), "", "K: and the board is back")

        # ★ The global search is typed into, behind a mode that says it is on —
        # otherwise the letter shortcuts would eat the text.
        assert_eq(inv(tf, "key", "c"), True, "K: `c` is a shortcut outside the box")
        # From empty, so the assertion below is about what was TYPED rather
        # than about whatever earlier sections left in the box.
        tf.intervene(f"{EXT}/search", "")
        assert_eq(inv(tf, "key", "/"), True, "K: `/` opens the search")
        for letter in "syn":
            assert_eq(inv(tf, "key", letter), True, f"K: {letter!r} is text now")
        assert_eq(q(tf, "search"), "syn", "K: which lands in the search box")
        assert_eq(inv(tf, "key", "Backspace"), True, "K: backspace deletes")
        assert_eq(q(tf, "search"), "sy", "K: one character")
        assert_eq(inv(tf, "key", "Escape"), True, "K: and Escape leaves the box")
        assert_eq(inv(tf, "key", "c"), True, "K: after which `c` is a shortcut again")

        # The keymap is published, so an agent does not have to read the source
        # to find the chords a person can press.
        keymap = q(tf, "keymap")
        for chord in ["Arrow=", "Shift+Arrow=", "Enter=", "Escape=", "/="]:
            assert chord in keymap, f"K: {chord} is published: {keymap}"


run_demo("R1648 the analyzer shell is assembled", body)
