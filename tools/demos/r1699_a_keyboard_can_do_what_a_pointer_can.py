#!/usr/bin/env python3
"""R1699 §5.38 §5.39 §5.40 §2 #2 #7 — **a keyboard can do what a pointer can.**

R1696 gave the two analysis screens one Tab stop per composite. R1698 gave each
composite a cursor its arrows move. Both were real, and driving the result
found that between them they had built a cursor a reader can walk and **never
act on**:

* **eleven Tab stops across the two screens, `Enter` and `Space` at every one of
  them, twenty-two presses that changed nothing painted.** Four of those stops
  declare `Activation::Explicit`, whose documented meaning is "arriving only
  moves the cursor; Enter or Space chooses" — nothing anywhere implemented the
  second half. Three more announce `role=button`, which a keyboard could not
  press: below the floor rather than above it, since a push button there
  activates on both keys, always;
* and the nested composite each screen has was **reachable with no way in**. The
  dashboard's application bar passes over its tab list in one step (WAI-ARIA's
  nesting, and correct) and no key descended, so from a keyboard the two views
  could not be switched at all. The capture viewer announces its message list as
  a `grid` whose sixteen rows each report seven cells, and `ArrowRight` on a row
  moved nothing — the columns existed for a reader and were unreachable by one.

So this drives the repaired keyboard through real windows: choosing at every
cursor position, entering and leaving a nested composite, the keys a composite
must still decline, and the roster a nested member publishes before anybody
descends into it.

Floor, measured by building a probe at 6.11.1 and running it offscreen rather
than by reading documentation: the bar-containing-a-tab-list arrangement is
FOUR Tab stops rather than one, an arrow from a bar control moves focus into the
tab bar while the opposite arrow walks straight out of the bar entirely, and
Escape does nothing anywhere. Its item view is better — one stop, both axes
moving a cell cursor, the focused cell named by its accessibility interface —
but it has no notion of a row being a unit with an inside, and Tab inside it
moves between cells instead of leaving the widget.

Run from the workspace root:
    cargo build -p hello-analyzer-shell -p hello-packet-view --release
    python3 tools/demos/r1699_a_keyboard_can_do_what_a_pointer_can.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, assert_eq, run_demo  # noqa: E402

EXT = "/external"
CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def tree(app: RpcSubprocess) -> tuple[dict, dict]:
    res = app.request("scene/access").result
    return {n["tag"]: n for n in res["nodes"]}, (res.get("focus") or {})


def cursor(app: RpcSubprocess) -> str | None:
    _, focus = tree(app)
    return focus.get("active_descendant")


def stops(app: RpcSubprocess, limit: int = 14) -> list[str]:
    """Walk the Tab ring once, the way a keyboard does."""
    seen: list[str] = []
    for _ in range(limit):
        app.request("focus/next")
        app.tick(16)
        _, focus = tree(app)
        tag = focus.get("tag")
        if tag is None or tag in seen:
            break
        seen.append(tag)
    return seen


def painted(app: RpcSubprocess) -> str:
    """Every painted tag and where — the set a press can change."""
    return repr(app.request("scene/tag_rects").result)


def go_to(app: RpcSubprocess, destination: str) -> None:
    """Take the screen to `destination` through its own rail, from the wire.

    `"*"` means chrome — painted at every destination, so nothing to do.
    """
    if destination == "*" or app.query(f"{EXT}/nav") == destination:
        return
    app.intervene(f"{EXT}/nav", destination)
    app.tick(16)
    assert_eq(app.query(f"{EXT}/nav"), destination, f"the rail reached {destination}")


def dashboard(app: RpcSubprocess) -> None:
    banner("A — every composite publishes the keys that choose and the keys that enter")
    walked = stops(app)
    ok("A: the ring is walked by Tab", len(walked) >= 4)
    composites = 0
    for stop in walked:
        app.request("focus/set", {"tag": stop})
        app.tick(16)
        nodes, _ = tree(app)
        nav = nodes[stop].get("navigation")
        if nav is None:
            continue
        composites += 1
        assert_eq(
            nav["choose_keys"],
            ["Enter", "Space"],
            f"A: {stop} declares Explicit, so it publishes the keys that choose",
        )
        assert_eq(nav["exit_key"], "Escape", f"A: {stop} publishes the key that leaves")
        ok(f"A: {stop} publishes a key that enters", "Enter" in nav["entry_keys"])
        # ★ The derivation that lets `entry_keys` be published rather than
        # declared: a key cannot both move this cursor and descend into a member.
        overlap = sorted(set(nav["entry_keys"]) & set(nav["keys"]))
        assert_eq(overlap, [], f"A: ★ {stop} navigates and enters by disjoint keys")
        assert_eq(nav["entered"], False, f"A: {stop} — nobody has descended yet")
    ok("A: four composites were asked", composites >= 4)
    print(f"[demo] dashboard: {len(walked)} stop(s), {composites} composite(s)")

    banner("B — ★ Enter at EVERY cursor position does something (22 presses did not, before)")
    # ★ Each stop is driven at a destination that SHOWS it. The first draft was
    # not, and the rail's own members are what moved the journey: by the time the
    # walk reached the layout bar the screen was somewhere else and the bar was
    # not focusable at all. A gate driving a control the screen is not showing is
    # asking a question with no answer.
    lives_at = {row["tag"]: row["at"] for row in app.query(f"{EXT}/spec")["focus_ring"]}
    silent = []
    chosen = 0
    for stop in walked:
        go_to(app, lives_at.get(stop, "*"))
        app.request("focus/set", {"tag": stop})
        app.tick(16)
        nodes, _ = tree(app)
        nav = nodes[stop].get("navigation")
        if nav is None:
            continue
        members = nav["members"]
        app.key(path=stop, name="Home")
        app.tick(16)
        for index, member in enumerate(members):
            if index:
                app.key(path=stop, name=nav["keys"][0])
                app.tick(16)
            if member["composite"]:
                continue
            assert_eq(cursor(app), member["tag"], f"B: the walk reached {member['tag']}")
            # ★ A reader can tell EITHER because the screen repainted or because
            # it said something. Both halves are load-bearing: the layout-preset
            # button opens a menu and flips its own `aria-expanded`, which is the
            # announcement WAI-ARIA specifies and not a toast; a booked palette
            # entry refuses, which paints nothing and is entirely the sentence.
            before = (painted(app), app.query(f"{EXT}/toast"))
            app.key(path=stop, name="Enter")
            app.tick(16)
            after = (painted(app), app.query(f"{EXT}/toast"))
            if after == before:
                silent.append(f"{stop} · {member['tag']}")
            chosen += 1
    assert_eq(silent, [], "B: ★ every member did something a reader can tell")
    ok("B: at least twenty-four members were chosen", chosen >= 24)
    print(f"[demo] {chosen} member(s) chosen from the keyboard, {len(silent)} silent")

    banner("C — ★ choosing a rail seat NAVIGATES, which a keyboard could not do")
    app.request("focus/set", {"tag": "shell.rail"})
    app.tick(16)
    app.key(path="shell.rail", name="Home")
    app.tick(16)
    here = app.query(f"{EXT}/nav")
    nodes, _ = tree(app)
    seats = [m for m in nodes["shell.rail"]["navigation"]["members"] if m["enabled"]]
    target = next(m for m in seats if not m["tag"].endswith(f".{here}"))
    while cursor(app) != target["tag"]:
        app.key(path="shell.rail", name="ArrowDown")
        app.tick(16)
    app.key(path="shell.rail", name="Enter")
    app.tick(16)
    there = app.query(f"{EXT}/nav")
    assert_eq(there != here, True, "C: ★ Enter on a rail seat arrived at that destination")
    print(f"[demo] rail: {here} -> {there} by keyboard alone")
    # Back, so the rest of the run is at the destination the screen opens on.
    while app.query(f"{EXT}/nav") != here:
        app.key(path="shell.rail", name="Home")
        app.tick(16)
        app.key(path="shell.rail", name="Enter")
        app.tick(16)

    banner("D — a booked seat REFUSES rather than doing nothing quietly")
    app.request("focus/set", {"tag": "shell.palette"})
    app.tick(16)
    nodes, _ = tree(app)
    booked = [m for m in nodes["shell.palette"]["navigation"]["members"] if not m["enabled"]]
    ok("D: the palette has booked entries", len(booked) > 0)
    app.key(path="shell.palette", name="Home")
    app.tick(16)
    while cursor(app) != booked[0]["tag"]:
        app.key(path="shell.palette", name="ArrowDown")
        app.tick(16)
    before = app.query(f"{EXT}/toast")
    app.key(path="shell.palette", name="Enter")
    app.tick(16)
    assert_eq(
        app.query(f"{EXT}/toast") != before,
        True,
        "D: ★ choosing a booked entry says why it refuses",
    )
    print(f"[demo] booked: {booked[0]['tag']} -> {app.query(f'{EXT}/toast')!r}")

    banner("E — ★ the nested tab list is entered, walked, chosen and left")
    app.request("focus/set", {"tag": "shell.appbar"})
    app.tick(16)
    app.key(path="shell.appbar", name="Home")
    app.tick(16)
    nodes, _ = tree(app)
    bar = nodes["shell.appbar"]["navigation"]
    nested = [m["tag"] for m in bar["members"] if m["composite"]]
    assert_eq(len(nested), 1, "E: the bar has exactly one member that is a composite")
    assert_eq(cursor(app), nested[0], "E: and the cursor is on it")

    inner = nodes[nested[0]].get("navigation")
    ok("E: ★ a nested composite publishes what its own arrows reach", inner is not None)
    ok("E: with more than one member", len(inner["members"]) >= 2)
    assert_eq(inner["entered"], False, "E: and says nobody is inside it")

    app.key(path="shell.appbar", name=bar["entry_keys"][0])
    app.tick(16)
    assert_eq(
        cursor(app),
        inner["members"][0]["tag"],
        "E: ★ entering lands on the tab list's own cursor",
    )
    nodes, _ = tree(app)
    assert_eq(
        nodes["shell.appbar"]["navigation"]["entered"],
        True,
        "E: and the wire says the reader has descended",
    )
    assert_eq(
        nodes["shell.appbar"]["navigation"]["cursor_tag"],
        nested[0],
        "E: while the bar's own cursor stayed on the tab list",
    )

    app.key(path="shell.appbar", name=inner["keys"][0])
    app.tick(16)
    assert_eq(cursor(app), inner["members"][1]["tag"], "E: the INNER axis moves between tabs")
    before_tab = app.query(f"{EXT}/tab")
    app.key(path="shell.appbar", name="Enter")
    app.tick(16)
    assert_eq(
        app.query(f"{EXT}/tab") != before_tab,
        True,
        "E: ★ Enter inside the tab list switched the view — impossible before this round",
    )

    app.key(path="shell.appbar", name="Escape")
    app.tick(16)
    assert_eq(cursor(app), nested[0], "E: Escape leaves ONE level, back onto the list")
    nodes, _ = tree(app)
    assert_eq(nodes["shell.appbar"]["navigation"]["entered"], False, "E: and says so")
    app.key(path="shell.appbar", name=bar["keys"][0])
    app.tick(16)
    assert_eq(
        cursor(app), bar["members"][1]["tag"], "E: and the bar's own axis answers again"
    )

    banner("F — ★ the off-axis arrow is consumed ONLY where there is something to enter")
    board = app.query(f"{EXT}/selected")
    before = painted(app)
    app.key(path="shell.appbar", name="ArrowDown")
    app.tick(16)
    assert_eq(
        cursor(app),
        bar["members"][1]["tag"],
        "F: ArrowDown at a plain member moved no cursor",
    )
    assert_eq(
        app.query(f"{EXT}/selected"),
        board,
        "F: ★ and did not reach the board the reader has left "
        "(R1698's invariant, narrowed rather than broken)",
    )
    assert_eq(painted(app), before, "F: nothing on the screen moved at all")

    banner("G — the account chip is a group, and it is not a keyboard destination")
    nodes, _ = tree(app)
    assert_eq(
        nodes["shell.rail.account"]["role"],
        "group",
        "G: ★ nothing presses it from either channel, so it does not announce an action",
    )
    rail = nodes["shell.rail"]["navigation"]
    ok(
        "G: and the rail's cursor walks destinations only",
        all(m["tag"] != "shell.rail.account" for m in rail["members"]),
    )


def capture(app: RpcSubprocess) -> None:
    banner("H — ★ a message row is entered and its cells are walked (the grid pattern)")
    walked = stops(app)
    # R1708 — by name, not by count. See the sibling note in
    # `r1698_a_composite_has_a_cursor_inside_it.py`: a hand-written `6` reported
    # R1707's new query field as a regression rather than as the stop it is.
    assert_eq(
        walked,
        [
            "pv.filter.query",
            "pv.filter.saved.0",
            "pv.filter.saved.1",
            "pv.filter.saved.2",
            "pv.list",
            "pv.tree",
            "pv.bytes",
        ],
        "H: the capture viewer's Tab ring, by name",
    )
    app.request("focus/set", {"tag": "pv.list"})
    app.tick(16)
    row = app.query(f"{EXT}/selected_row")
    assert_eq(cursor(app), f"pv.list.row.{row}", "H: the cursor opens on the row")

    nodes, _ = tree(app)
    nav = nodes["pv.list"]["navigation"]
    composites = [m for m in nav["members"] if m["composite"]]
    assert_eq(
        len(composites),
        len(nav["members"]),
        "H: ★ EVERY row is a composite — that is what `grid` means",
    )
    inner = nodes[f"pv.list.row.{row}"].get("navigation")
    ok("H: ★ a row publishes the cells its arrows reach", inner is not None)
    cells = [m["tag"] for m in inner["members"]]
    ok("H: seven columns", len(cells) == 7)

    app.key(path="pv.list", name=nav["entry_keys"][0])
    app.tick(16)
    assert_eq(cursor(app), cells[0], "H: ★ entering a row lands on its first cell")
    app.key(path="pv.list", name="ArrowRight")
    app.tick(16)
    assert_eq(cursor(app), cells[1], "H: the inner axis walks the columns")
    app.key(path="pv.list", name="End")
    app.tick(16)
    assert_eq(cursor(app), cells[-1], "H: End reaches the last column")
    # ★ The ends policy is tested by an ADVANCE past the last member, not by
    # pressing End twice: `End` lands on the last index whatever the policy is,
    # so that assertion could not fail and a counterfactual flipping this row to
    # `Wrap` walked straight through it.
    app.key(path="pv.list", name="ArrowRight")
    app.tick(16)
    assert_eq(
        cursor(app),
        cells[-1],
        "H: ★ and an advance past it STOPS — a row is not a ring, unlike the tab list",
    )
    assert_eq(
        app.query(f"{EXT}/selected_row"),
        row,
        "H: walking the cells did not change which message is decoded",
    )

    nodes, _ = tree(app)
    current = [
        t
        for t, n in nodes.items()
        if t.startswith("pv.list.cell.") and (n.get("state") or {}).get("focused")
    ]
    assert_eq(current, [cells[-1]], "H: ★ exactly one cell is current, and it is that one")

    app.key(path="pv.list", name="Escape")
    app.tick(16)
    assert_eq(cursor(app), f"pv.list.row.{row}", "H: Escape leaves the row, not the pane")
    app.key(path="pv.list", name="ArrowDown")
    app.tick(16)
    assert_eq(
        app.query(f"{EXT}/selected_row"),
        row + 1,
        "H: and the pane's own axis moves between rows again",
    )

    banner("I — ★ a filter chip is pressed from the keyboard (a button that could not be)")
    for n in range(3):
        chip = f"pv.filter.saved.{n}"
        app.request("focus/set", {"tag": chip})
        app.tick(16)
        nodes, _ = tree(app)
        assert_eq(nodes[chip]["role"], "button", f"I: {chip} announces itself a button")
        assert_eq(nodes[chip].get("navigation"), None, f"I: {chip} owns no cursor")

        # ★ The observable is the chip's own `aria-pressed`, not the painted
        # rectangles: a saved filter turning on changes the chip's FILL and moves
        # nothing, so a gate watching geometry would have reported a button that
        # works as a button that does not.
        def pressed() -> bool:
            nodes, _ = tree(app)
            return (nodes[chip].get("state") or {}).get("checked")

        before = pressed()
        app.key(path=chip, name="Enter")
        app.tick(16)
        assert_eq(pressed() != before, True, f"I: ★ Enter pressed {chip}")
        app.key(path=chip, name="Space")
        app.tick(16)
        assert_eq(pressed(), before, "I: and Space pressed it back — one verb, two keys")

    banner("J — and an arrow at a chip still reaches nothing it should not")
    row_before = app.query(f"{EXT}/selected_row")
    for n in range(3):
        chip = f"pv.filter.saved.{n}"
        app.request("focus/set", {"tag": chip})
        app.tick(16)
        app.key(path=chip, name="ArrowDown")
        app.tick(16)
    assert_eq(
        app.query(f"{EXT}/selected_row"),
        row_before,
        "J: standing on a chip, ArrowDown moves no message list",
    )


def body() -> None:
    with RpcSubprocess("hello-analyzer-shell", boot_grace=1.5) as app:
        dashboard(app)
    with RpcSubprocess("hello-packet-view", boot_grace=1.5) as app:
        capture(app)
    print(f"\n[demo] {len(CHECKS)} narrated check(s) beyond the assertions")


run_demo("R1699 a keyboard can do what a pointer can", body)
