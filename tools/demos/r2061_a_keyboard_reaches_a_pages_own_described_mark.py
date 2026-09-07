#!/usr/bin/env python3
"""R2061 §5.38 §5.40 §5.39 §5.12 §2 #7 — **a reader with no pointer reaches a
mark the PAGE describes, and is told what it is for.**

# What this demo exists for

R1918 gave all six pages of the assembled tool a described mark and measured,
in the same round, that four of them were out of a keyboard reader's reach:
what such a reader could stand on at `packets`, `keys`, `logs` and `lab` was
the shell's chrome, and the sentence they were shown was the rail seat's ⇒
`debt-a-described-mark-is-out-of-a-keyboard-readers-reach`.

Re-measured this round by walking the ring with `focus/next` before touching
anything, which is what that debt asks its repaying round to do first. Two
facts, neither of them read off the source:

  * at all four pages every ring stop showed NOTHING, and the only sentence a
    keyboard could reach anywhere was `Go to Dashboard` — the rail's.
  * the reason is not only that the marks are not stops. The screens were
    asking their register *which mark is the reader resting on* with the STOP,
    and no mark that carries a sentence IS a stop — each lives inside one. The
    shell learned that at R1918; its mounted screens did not.

This walk holds the pages repaid so far. R2061 was the first — the capture
list's column headings — and R2063 added the key section, whose heading row was
painted as a panel the accessibility tree called by another name, and whose
record pane holds an action a keyboard could press and never hear about.

# ★★★★★ Why the heading row and not the marks

The seven headings cannot each be a Tab stop — that is seven stops for one row,
and the composite pattern exists precisely so a row costs one. So the row is the
stop and the arrows move inside it, which is also what the accessibility tree
has announced since R1694: a `row` of `columnheader`s. Until this round nothing
painted that row, so the tree described a room with no floor.

# ⚠ The population is derived, never listed here

Which marks belong to the page comes from the mounted screen's own register
(`/packet_view/external/described`), and which stops exist comes from walking
the ring. A list of seven headings written here would be a second spelling of
the screen's column table, and an eighth column would leave this walk silently
complete.

# What this walk holds

  (A) with no pointer and nothing focused, the page shows no description — the
      control, without which a screen mounting one permanently would pass.
  (B) ★ walking the ring with the KEYBOARD ALONE reaches a stop whose sentence
      is one of the PAGE's own marks — not the chrome's. This is the debt's
      closing condition for this page.
  (C) the arrows move along the row and the sentence follows, so the row is
      walkable rather than a single door.
  (D) walking it REORDERS NOTHING — the row declares explicit activation, and a
      row that sorted once per arrow would run six orderings on the way to the
      seventh column.
  (E) but `Enter` does reorder, from the keyboard, at the column the cursor is
      on: a heading a reader can reach and cannot press is a control below the
      floor rather than above it.
  (F) the mark POINTS AT the sentence, so an assistive technology reads it out.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 \\
        tools/demos/r2061_a_keyboard_reaches_a_pages_own_described_mark.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"

#: The destinations repaid so far, one per instalment.
#:
#: ⚠ A written list, deliberately, and it is what makes this walk's claim exact:
#: the debt is repaid one page at a time, so a walk that asked the whole roster
#: would fail on the pages nobody has reached yet and a walk that asked "any
#: page" would go quiet the moment one regressed. Each name here is a page a
#: round claimed. ★ When the last one lands this becomes the roster itself —
#: `/external/destinations` — and stops being a list at all.
REPAID = ("packets", "keys", "logs", "lab")

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def access(app: RpcSubprocess) -> list[dict]:
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result.get("nodes", [])


def tooltips(app: RpcSubprocess) -> list[dict]:
    # ⚠ The wire spells the role LOWERCASE — the WAI-ARIA spelling rather than
    # the Rust variant's, a distinction R1916's walk recorded after a first
    # draft reported zero on a screen that was publishing one.
    return [n for n in access(app) if n.get("role") == "tooltip"]


def focused(app: RpcSubprocess) -> str | None:
    resp = app.request("focus/get")
    return (resp.result or {}).get("focused") if resp and resp.result else None


def destination(app: RpcSubprocess, key: str) -> dict:
    rows = js(app.query(f"{EXT}/destinations"))["destinations"]
    row = next((r for r in rows if r["key"] == key), None)
    assert row is not None, f"the roster has no destination {key}"
    return row


def page_register(app: RpcSubprocess, row: dict) -> dict[str, str]:
    """The mounted page's OWN described marks, as `{tag: sentence}`.

    The host's `chrome` half is deliberately not merged in: the whole point of
    the debt is that a reader was reaching only that half, so a walk that could
    not tell the two apart could not see the defect it exists to refuse.
    """
    theirs = js(app.query(f"{row['screen']['address']}/described"))
    return {m["tag"]: m["sentence"] for m in theirs["marks"]}


def shown(app: RpcSubprocess) -> tuple[str | None, str | None]:
    """`(the mark being described, the sentence)` — from the ANNOUNCEMENT.

    Read out of the accessibility tree rather than from the register, because
    the register is what the screen *could* say and this walk is about what a
    reader *is told*.
    """
    tips = tooltips(app)
    if len(tips) != 1:
        return None, None
    region = tips[0].get("tag")
    anchor = next((n for n in access(app) if n.get("described_by") == region), None)
    return (anchor or {}).get("tag"), tips[0].get("name")


def press(app: RpcSubprocess, stop: str, chord: str) -> None:
    """Send `chord` to `stop` and put the pointer back where it was: nowhere.

    ★★★★★ R2063 — the harness's `path=` form of `scene/key` resolves the tag's
    painted rectangle and uses its CENTRE as the event's cursor target, so every
    press quietly moves the pointer onto the row and marks the screen as being
    pointed at. This walk's whole premise is a reader with no pointer, and a
    register that prefers a hover to a focus — which is the conventional order —
    then answers about whatever the row's centre happens to sit on.

    Measured: the cursor walked all seven headings correctly while the sentence
    stayed frozen on the column under the row's midpoint. The screen was right
    and the instrument was moving the thing it measured. A single arrow step
    could not see it, because the frozen answer was a legal one.
    """
    app.key(path=stop, name=chord)
    app.tick_ms(16)
    app.pointer_leave()
    app.tick_ms(16)


def owns(nodes: dict, stop: str, mark: str) -> bool:
    """Whether the announced tree puts `mark` anywhere inside `stop`."""
    seen: set[str] = set()
    frontier = [stop]
    while frontier:
        here = frontier.pop()
        if here in seen:
            continue
        seen.add(here)
        children = nodes.get(here, {}).get("children") or []
        if mark in children:
            return True
        frontier.extend(children)
    return False


def ring_of(app: RpcSubprocess) -> set[str]:
    """The stops that exist right now.

    ⚠ By WALKING, not by reading a `focusable` field off the paint: a first
    draft did the latter and the wait never fired, because the flag is not what
    the snapshot carries under that name. Walking uses the one surface that is
    known to answer — the same `focus/next` every other clause here trusts —
    and the caller sets focus straight afterwards, so disturbing it costs
    nothing.
    """
    return set(walk_ring(app))


def walk_ring(app: RpcSubprocess, limit: int = 40) -> list[str]:
    """Every stop the keyboard reaches from here, in Tab order."""
    ring: list[str] = []
    for _ in range(limit):
        app.request("focus/next")
        app.tick_ms(16)
        here = focused(app)
        if here is None or here in ring:
            break
        ring.append(here)
    return ring


def repay(app: RpcSubprocess, page: str) -> tuple[dict, str]:
    """Hold (A)-(C) and (F) at one repaid destination.

    Returns the destination row and the stop the page's own mark was reached
    from, so a page with a clause of its own can carry on from there.
    """
    row = destination(app, page)
    app.intervene(f"{EXT}/nav", page)
    app.tick_ms(16)
    ok(f"{page}: the journey reaches it", app.query(f"{EXT}/nav") == page)

    marks = page_register(app, row)
    ok(f"{page}: it publishes described marks of its own — {len(marks)}", len(marks) >= 1)

    # (A) the control. A pointer that never left would make every later
    # sentence a hover's, and a screen that mounted a description
    # permanently would pass the whole of (B).
    banner(f"{page} A — nothing is shown before a reader arrives")
    app.pointer_leave()
    app.request("focus/set", {"tag": None})
    app.tick_ms(16)
    ok(f"A {page}: no description is announced", not tooltips(app))

    # (B) the closing condition for this page: the KEYBOARD alone.
    banner(f"{page} B — the keyboard walks to a mark the page describes")
    ring = walk_ring(app)
    ok(f"{page}: the ring has stops here — {len(ring)}", len(ring) >= 2)
    print("    ring: " + ", ".join(ring))

    reached: list[tuple[str, str, str]] = []
    for stop in ring:
        app.request("focus/set", {"tag": stop})
        app.tick_ms(16)
        mark, sentence = shown(app)
        # ★★★★★ A composite may hold its described marks ONE LEVEL DOWN, and a
        # survey that only lands on stops cannot see them. The node lab is the
        # case that forced this: its canvas holds cards and a card holds the
        # pins that carry the sentences, so standing on the canvas shows
        # nothing and the page read as zero of eighteen. A reader does not stop
        # there either — they press the key the stop PUBLISHES as its way in.
        if mark not in marks:
            nav = {n.get("tag"): n for n in access(app)}.get(stop, {}).get("navigation") or {}
            # ⚠ Only a stop whose MEMBERS are composites, and only then. A first
            # draft pressed the entry key at every silent stop and the walk
            # broke three clauses later on a page it had already left — pressing
            # keys into rows and chip bars that were never the subject changed
            # the screen underneath the assertions that follow. The question is
            # "does anything nest here", and the roster answers it.
            nests = any(m.get("composite") for m in nav.get("members") or [])
            for key in (nav.get("entry_keys") or []) if nests else []:
                press(app, stop, key)
                mark, sentence = shown(app)
                if mark in marks:
                    break
        if mark in marks:
            reached.append((stop, mark, sentence or ""))
    ok(
        f"B {page}: ★★★★★ a reader with NO POINTER reaches a mark this page "
        f"describes — {len(reached)} of {len(ring)} stop(s)",
        len(reached) >= 1,
    )
    stop, mark, sentence = reached[0]
    print(f"    {stop} -> {mark}: {sentence!r}")
    ok(
        f"B {page}: and the sentence is the register's own for {mark}",
        sentence == marks[mark],
    )
    # ★★★★★ The mark lives INSIDE the stop, and that is asked of the ANNOUNCED
    # TREE rather than of the tags' spelling. A first draft compared prefixes —
    # `pv.list.head.0` does sit under `pv.list` — and the second page disproved
    # it at once: this section's heading row is `kp.list.header` and its
    # headings are `kp.column.*`, two families for one containment, which is
    # perfectly legal. A predicate written from ONE example is a predicate about
    # that example.
    app.request("focus/set", {"tag": stop})
    app.tick_ms(16)
    nodes = {n.get("tag"): n for n in access(app)}
    ok(f"B {page}: the stop is not the mark ({mark} vs {stop})", mark != stop)
    # ⚠ INSIDE, at any depth — not "a direct child". A composite may nest: the
    # node lab's canvas holds cards and a card holds the pins that carry the
    # sentences, so the mark is a grandchild there. A first draft asked for
    # direct childhood and reported that two-level containment as a defect.
    ok(
        f"B {page}: and the tree says the mark is INSIDE the stop — "
        f"{stop} owns {mark}",
        owns(nodes, stop, mark),
    )
    ok(
        f"B {page}: ★ and the mark is the one the tree marks focused, which is "
        "what an assistive technology follows",
        (nodes.get(mark, {}).get("state") or {}).get("focused") is True,
    )

    # (F) the pointing half, asserted where the reader is standing — and
    # the walk has to GO BACK there. Surveying the ring left focus on its
    # last stop, so asking the tree now would ask about the chrome; the
    # first draft did exactly that and reported the mark unreferenced.
    app.request("focus/set", {"tag": stop})
    app.tick_ms(16)
    anchor = next((n for n in access(app) if n.get("tag") == mark), None)
    ok(f"F {page}: the mark itself is announced — {mark}", anchor is not None)
    ok(
        f"F {page}: ★ and it POINTS AT the description; a region nothing "
        "references is a region an assistive technology never reads out",
        anchor.get("described_by") == tooltips(app)[0].get("tag"),
    )

    # (C) the row is WALKABLE — one door is not a row.
    banner(f"{page} C — the arrows move along the row and the sentence follows")
    app.request("focus/set", {"tag": stop})
    app.tick_ms(16)
    before = shown(app)
    # ⚠ The keys the stop PUBLISHES, not `ArrowRight` assumed. Three of these
    # pages hold their marks in a horizontal row and the fourth holds them in a
    # vertical one inside a card, so a walk that named one arrow reported the
    # lab's cursor as not moving at all. Ask; the roving says which keys it
    # navigates by.
    arrows = (
        {n.get("tag"): n for n in access(app)}.get(stop, {}).get("navigation", {}).get("keys")
        or ["ArrowRight"]
    )
    after = before
    for key in arrows:
        press(app, stop, key)
        after = shown(app)
        if after[0] != before[0]:
            break
    ok(
        f"C {page}: an arrow the stop publishes moves to another mark — "
        f"{before[0]} -> {after[0]} (tried {arrows})",
        after[0] != before[0],
    )
    ok(f"C {page}: which is also this page's — {after[0]}", after[0] in marks)
    ok(f"C {page}: and its own sentence comes with it", after[1] == marks[after[0]])

    # ★★★★★ And the WHOLE of EVERY such row, not one step of one of them. A
    # single arrow proves the cursor moves; it does not prove every member is
    # somewhere a reader can get to, which is the difference between "the method
    # works" and "the population is repaid" — the distinction this campaign
    # keeps paying for.
    #
    # ⚠ Every stop that showed a mark, because a page can have more than one:
    # the log section's severity row and its heading row are both composites,
    # and a walk that only opened the first reported four of eight and read like
    # a screen defect. The roster of each comes from the announced tree, so a
    # column added to a screen joins this without anybody remembering.
    covered: set[str] = set()
    for at, _, _ in reached:
        # ⚠ The ROSTER the arrows reach, which is not the announced children. A
        # stop that publishes no navigation is a single control, and its
        # children are structure rather than places a cursor goes — the record
        # pane announces ten parts and exactly one of them is a described mark,
        # which a walk over `children` reported as one of ten missing.
        nav = nodes.get(at, {}).get("navigation")
        if not nav:
            continue
        members = [m["tag"] for m in nav.get("members") or []]
        arrows = nav.get("keys") or ["ArrowRight", "ArrowLeft"]
        # ★★★★★ THE LEAVES, not the members. A roster's members may themselves
        # be composites — the node lab's canvas holds cards and a card holds the
        # pins that carry the sentences — so the places a reader can END UP are
        # one level down there and are the members themselves everywhere else.
        # Taken from what each member PUBLISHES, so the population is the
        # screen's answer rather than this walk's guess.
        leaves: list[str] = []
        for member in members:
            inner = nodes.get(member, {}).get("navigation") or {}
            below = [m["tag"] for m in inner.get("members") or []]
            leaves.extend(below or [member])
        # ⚠ Only the leaves that ARE described marks. A first draft compared
        # against every leaf and the assertion became unsatisfiable: three of
        # the node lab's cards draw no pin that speaks, so their own tag is the
        # leaf and no reader could ever be "shown its sentence". The claim this
        # clause is for is that nothing the register describes is out of reach —
        # not that every place a cursor stops has something to say.
        leaves = [leaf for leaf in leaves if leaf in marks]
        seen: dict[str, str] = {}
        app.request("focus/set", {"tag": at})
        app.tick_ms(16)
        # ⚠ NO notion of "the start". A first draft walked to one end first and
        # got the direction wrong — the published key order puts the forward
        # arrow first, so "go to the start" went to the END and the clause
        # reported two of seven. These rosters stop at their ends rather than
        # wrapping, so what covers one regardless of where the cursor happens
        # to be is: press each published arrow until it stops moving, for every
        # arrow. Both directions and both axes, without knowing which is which.
        entry = nav.get("entry_keys") or []
        exit_key = nav.get("exit_key")

        def collect() -> None:
            here, sentence = shown(app)
            if here in marks:
                seen[here] = sentence or ""

        def sweep_axes(limit: int) -> None:
            """Press each published arrow until it stops moving, both ways."""
            for key in arrows:
                for _ in range(limit + 2):
                    collect()
                    before_tag = shown(app)[0]
                    press(app, at, key)
                    if shown(app)[0] == before_tag:
                        break
                collect()

        if leaves == members:
            # The flat shape: the members ARE the marks.
            sweep_axes(len(leaves))
        else:
            # ★★★★★ The nesting shape, and it needs the THIRD published key.
            # Arrows move inside whichever member the cursor entered, so a walk
            # that only pressed arrows and the entry key stayed in the first
            # card and reported two of eight. Getting to another card's leaves
            # means LEAVING this one — which the roster also publishes, as its
            # exit key. ⇒ ask a composite for all three: what moves, what goes
            # in, what comes out.
            # ⚠ Bounded by PROGRESS, not by a member count. A first draft did one
            # exit-arrow-enter cycle per member and reached four cards of eight,
            # because where the cursor started and where the ends are is not
            # something the walk knows. So it cycles until a whole pass adds
            # nothing new — and each pass tries BOTH directions at the member
            # level, so neither end can hide the rest.
            for direction in list(arrows) or [None]:
                for _ in range(len(members) + 2):
                    was = len(seen)
                    for key in entry:
                        press(app, at, key)
                    sweep_axes(len(leaves))
                    if exit_key:
                        press(app, at, exit_key)
                    if direction:
                        press(app, at, direction)
                    if len(seen) == len(leaves):
                        break
                    if len(seen) == was and direction is None:
                        break
                if len(seen) == len(leaves):
                    break
        # ★ Names the ones it could not reach. A census that answers HOW MANY
        # cannot answer WHICH, and this tree wrote that lesson down two rounds
        # after building a gate that only counted.
        missed = sorted(set(leaves) - set(seen))
        print(f"    walked {at}: {len(seen)} of {len(leaves)} — missed {missed}")
        ok(
            f"C {page}: ★★★★★ EVERY leaf of {at} is a mark a reader reaches — "
            f"{len(seen)} of {len(leaves)}, missed {missed}",
            not missed,
        )
        ok(
            f"C {page}: and each member of {at} says its own sentence",
            all(seen[tag] == marks[tag] for tag in seen),
        )
        covered |= set(seen)

    # ★★★★★ How much of the page's register a keyboard reaches, REPORTED rather
    # than judged — so "this page is repaid" is a fraction somebody can read
    # instead of a claim resting on the one mark clause (B) happened to find
    # first.
    covered |= {m for _, m, _ in reached}
    print(
        f"    [reach] {page}: {len(covered)} of {len(marks)} described mark(s) "
        f"are keyboard-reachable; out of reach: {sorted(set(marks) - covered)}"
    )
    return row, stop


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        roster = {r["key"] for r in js(app.query(f"{EXT}/destinations"))["destinations"]}
        ok(
            f"every page this walk claims is one the tool navigates to — {REPAID}",
            set(REPAID) <= roster,
        )
        held: dict[str, tuple[dict, str]] = {}
        for page in REPAID:
            held[page] = repay(app, page)

        # (D)/(E) the capture list's own clause: what its heading row does to
        # the capture, which is the reason its activation policy is explicit
        # rather than following. Only this page has an ordering to disturb —
        # the key section orders by no column at all, so asking it the same
        # question would be asking about a verb it does not have.
        row, stop = held["packets"]
        banner("packets D/E — walking reorders nothing; pressing does")
        # ⚠ Navigating BACK to a page needs the frames that MOUNT it before its
        # stops exist, and "arriving" is not "being drawn". A fixed tick count
        # is a guess about how long that takes, and it was wrong TWICE — one
        # tick sufficed while the page happened to be adjacent, four sufficed
        # until a heavier page went last. So the walk WAITS for the stop to be
        # focusable instead of counting frames.
        app.intervene(f"{EXT}/nav", "packets")
        for _ in range(8):
            app.tick_ms(16)
            if stop in ring_of(app):
                break
        app.request("focus/set", {"tag": stop})
        app.tick_ms(16)
        order = app.query(f"{row['screen']['address']}/sort")
        press(app, stop, "ArrowRight")
        press(app, stop, "ArrowLeft")
        ok(
            "D: ★★★★★ walking the row leaves the capture's order alone — "
            f"{order!r}",
            app.query(f"{row['screen']['address']}/sort") == order,
        )
        here = shown(app)[0]
        press(app, stop, "Enter")
        after_press = app.query(f"{row['screen']['address']}/sort")
        ok(
            f"E: ★★★★★ and pressing it DOES reorder, at {here} — "
            f"{order!r} -> {after_press!r}",
            after_press != order,
        )

        # (G) the log section's own clause, and it is the OPPOSITE policy —
        # deliberately, because the difference is the ROLE and not a preference.
        # That section's severity row announces itself as a radio group, and a
        # radio group's arrow SELECTS; a group whose arrows only moved a cursor
        # would announce one thing and do another. Asserted here because a
        # `Follows` cursor that quietly stopped writing would leave the walk's
        # other clauses entirely green.
        banner("logs G — a radio group's arrow chooses, because that is the role")
        logs = held["logs"][0]
        app.intervene(f"{EXT}/nav", "logs")
        for _ in range(4):
            app.tick_ms(16)
        # ★ The row is FOUND, not spelled: it is the stop on this page whose
        # published cursor FOLLOWS, which is the very property this clause is
        # about. A tag written out here would be a second copy of an address the
        # screen already composes, and a wrong letter in it would read as the
        # screen not painting the mark rather than as a typo.
        severity = None
        for stop_here in walk_ring(app):
            app.request("focus/set", {"tag": stop_here})
            app.tick_ms(16)
            nav = {n.get("tag"): n for n in access(app)}.get(stop_here, {}).get("navigation")
            if nav and nav.get("activation") == "follows":
                severity = stop_here
                break
        ok("G: this page has a row whose cursor FOLLOWS", severity is not None)
        app.request("focus/set", {"tag": severity})
        app.tick_ms(16)
        # ⚠ `Home` first, because the clause above walked this row to its end and
        # it STOPS there — an `ArrowRight` from the last member moves nothing,
        # which the first draft read as the cursor not writing at all. A walk
        # that assumes where it is standing is a walk asserting about the
        # previous clause.
        press(app, severity, "Home")
        chosen = app.query(f"{logs['screen']['address']}/severity")
        press(app, severity, "ArrowRight")
        moved = app.query(f"{logs['screen']['address']}/severity")
        ok(
            f"G: ★★★★★ the arrow CHOOSES — {chosen!r} -> {moved!r}",
            moved != chosen,
        )
        ok(
            f"G: and the row that did it is {severity}, whose members the tree "
            "announces as a choice rather than as places to visit",
            # ⚠ The wire spells this role `radio`, not the Rust variant's
            # `RadioButton` — the same lowercase/short spelling R1916 recorded
            # for the description region, and a draft comparing the Rust name
            # reported zero on a screen that was publishing three.
            all(
                n.get("role") == "radio"
                for n in access(app)
                if n.get("tag")
                in {
                    m["tag"]
                    for m in (
                        {k.get("tag"): k for k in access(app)}
                        .get(severity, {})
                        .get("navigation", {})
                        .get("members")
                        or []
                    )
                }
            ),
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r2061_a_keyboard_reaches_a_pages_own_described_mark", body))
