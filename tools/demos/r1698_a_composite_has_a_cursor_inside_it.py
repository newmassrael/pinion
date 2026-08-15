#!/usr/bin/env python3
"""R1698 §5.38 §5.39 §5.40 §2 #2 #7 — **a composite has a cursor inside it.**

WAI-ARIA's composite widget pattern is two halves. R1693 and R1696 gave the two
analysis screens the first: one Tab stop per composite, so a keyboard reaches a
toolbar in one press rather than in as many presses as it has controls. Nobody
had built the second — **inside** the composite an arrow moves a cursor between
the members, and the composite publishes where that cursor rests.

Measured by driving both running applications the day this round opened:

* eleven Tab stops between the two screens, four arrow keys each — **forty-four
  presses that moved nothing** — and an active descendant that was `None` at
  every one of them;
* the dashboard was worse than that. Its keymap of twelve chords was reachable
  only through the wire: `invoke("key", "ArrowRight")` moved the board's
  selection and a **real key press moved nothing at all**, because the screen
  implemented no `apply_key` hook. Across 225 examples, 172 bindings implement
  that hook and 135 of them read the `focused` argument; this screen implemented
  it zero times, so every test that drove its keyboard passed for the same
  reason R1693's did on the sibling — the test and the defect were the same
  mistake;
* and the capture viewer, whose keys DID arrive, dropped `focused`: at all six
  of its stops, including the decode tree and the byte grid, `ArrowDown` moved
  the **message list**. An arrow meant one thing no matter where anybody stood.

So this drives the repaired keyboard through real windows: every declared
composite, every arrow, `Home` and `End`, the keys a composite must decline, and
the roster it publishes — which is deliberately not its accessibility children.

Run from the workspace root:
    cargo build -p hello-analyzer-shell -p hello-packet-view --release
    python3 tools/demos/r1698_a_composite_has_a_cursor_inside_it.py
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


def tree(app: RpcSubprocess) -> dict:
    res = app.request("scene/access").result
    return {n["tag"]: n for n in res["nodes"]}, (res.get("focus") or {})


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


def cursor(app: RpcSubprocess) -> str | None:
    _, focus = tree(app)
    return focus.get("active_descendant")


def dashboard(app: RpcSubprocess) -> None:
    banner("A — the dashboard: a real key press reaches the screen at all")
    # ★ The measurement that opened the round, as an assertion. Before the
    # `apply_key` hook existed this moved nothing while the wire moved the
    # board, which is the whole class R1693 named.
    app.request("focus/set", {"tag": "shell.canvas"})
    app.tick(16)
    before = app.query(f"{EXT}/selected")
    app.key(path="shell.canvas", name="ArrowRight")
    app.tick(16)
    assert_eq(
        app.query(f"{EXT}/selected") != before,
        True,
        "A: ★ a REAL key press moves the board — it did not before this round",
    )

    banner("B — every composite the ring declares has a cursor its arrows move")
    spec = app.query(f"{EXT}/spec")
    declared = {row["tag"]: row for row in spec["focus_ring"]}
    walked = stops(app)
    ok("B: the ring is walked by Tab", len(walked) >= 4)
    checked = 0
    for stop in walked:
        app.request("focus/set", {"tag": stop})
        app.tick(16)
        nodes, _ = tree(app)
        nav = nodes[stop].get("navigation")
        if nav is None:
            # The board's cursor is spatial rather than a linear roster, so it
            # declares none — and still reports the card it is on.
            ok(f"B: {stop} has no roster and still names its cursor", cursor(app) is not None)
            continue
        checked += 1
        ok(f"B: {stop} publishes its members", len(nav["members"]) >= 2)
        assert_eq(
            nodes[stop].get("orientation"),
            {"horizontal": "horizontal", "vertical": "vertical", "both": None}[nav["axis"]],
            f"B: {stop} publishes the orientation its axis implies",
        )
        first = cursor(app)
        ok(f"B: {stop} rests its cursor on a member", first in [m["tag"] for m in nav["members"]])

        advance, retreat = nav["keys"][0], nav["keys"][len(nav["keys"]) // 2]
        app.key(path=stop, name=advance)
        app.tick(16)
        moved = cursor(app)
        assert_eq(moved != first, True, f"B: {stop}: {advance} moved the cursor")
        app.key(path=stop, name=retreat)
        app.tick(16)
        assert_eq(cursor(app), first, f"B: {stop}: {retreat} brought it back")

        # Home and End — the pair the reference toolkit's tab list implements
        # neither of, measured by building a probe and running it.
        app.key(path=stop, name="End")
        app.tick(16)
        assert_eq(cursor(app), nav["members"][-1]["tag"], f"B: {stop}: End reaches the last")
        app.key(path=stop, name="Home")
        app.tick(16)
        assert_eq(cursor(app), nav["members"][0]["tag"], f"B: {stop}: Home reaches the first")

        # ★ The off-axis arrow is DECLINED — it must not move this cursor, and
        # it must not move the board either.
        off = "ArrowUp" if nav["axis"] == "horizontal" else "ArrowLeft"
        if nav["axis"] != "both":
            was_board = app.query(f"{EXT}/selected")
            here = cursor(app)
            app.key(path=stop, name=off)
            app.tick(16)
            assert_eq(cursor(app), here, f"B: {stop}: {off} is off the axis and moved nothing")
            assert_eq(
                app.query(f"{EXT}/selected"),
                was_board,
                f"B: ★ {stop}: and it did not reach the board the reader has left",
            )
    ok("B: four composites were driven", checked >= 4)
    print(f"[demo] dashboard: {len(walked)} stop(s), {checked} with a roster")

    banner("C — the roster is what the arrows reach, not the container's children")
    app.request("focus/set", {"tag": "shell.palette"})
    app.tick(16)
    nodes, _ = tree(app)
    palette = nodes["shell.palette"]
    nav = palette["navigation"]
    assert_eq(
        len(nav["members"]),
        len(spec["catalogue"]),
        "C: the palette's cursor walks its catalogue entries",
    )
    ok(
        "C: ★ and NOT its children, which are its sections and two readouts",
        len(nav["members"]) != len(palette.get("children", [])),
    )
    for member in nav["members"]:
        ok(f"C: {member['tag']} is a node a reader can be told about", member["tag"] in nodes)
    locked = [m for m in nav["members"] if not m["enabled"]]
    ok("C: a booked entry is reachable by the cursor", len(locked) > 0)
    for m in locked:
        ok(f"C: {m['tag']} says it refuses", nodes[m["tag"]]["state"].get("disabled") is True)
    print(f"[demo] palette: {len(nav['members'])} member(s), {len(locked)} booked and reachable")

    banner("D — the policy is published, and it is not one policy for everything")
    ends = {row["tag"]: nodes[row["tag"]]["navigation"]["ends"]
            for row in spec["focus_ring"]
            if row["tag"] in nodes and nodes[row["tag"]].get("navigation")}
    ok("D: at least one composite wraps and one stops", len(set(ends.values())) == 2)
    print(f"[demo] ends policy: {ends}")


def capture(app: RpcSubprocess) -> None:
    banner("E — the capture viewer: each pane's arrows move that pane's cursor")
    walked = stops(app)
    ok("E: six stops", len(walked) == 6)
    panes = [s for s in walked if s in ("pv.list", "pv.tree", "pv.bytes")]
    assert_eq(len(panes), 3, "E: three panes own a cursor")

    for stop in panes:
        app.request("focus/set", {"tag": stop})
        app.tick(16)
        nodes, _ = tree(app)
        nav = nodes[stop].get("navigation")
        ok(f"E: {stop} publishes a roster", nav is not None and len(nav["members"]) >= 2)
        assert_eq(nav["activation"], "follows", f"E: {stop}'s cursor IS its selection")
        first = cursor(app)
        ok(f"E: {stop} names its cursor", first is not None)
        app.key(path=stop, name=nav["keys"][0])
        app.tick(16)
        assert_eq(cursor(app) != first, True, f"E: {stop}: the arrow moved its cursor")

    banner("F — ★ and a plain button owns no cursor, so an arrow there moves nothing")
    row_before = app.query(f"{EXT}/selected_row")
    for n in range(3):
        chip = f"pv.filter.saved.{n}"
        app.request("focus/set", {"tag": chip})
        app.tick(16)
        nodes, _ = tree(app)
        ok(f"F: {chip} publishes no roster", nodes[chip].get("navigation") is None)
        app.key(path=chip, name="ArrowDown")
        app.tick(16)
    assert_eq(
        app.query(f"{EXT}/selected_row"),
        row_before,
        "F: ★ standing on a filter chip, ArrowDown no longer moves the message list",
    )

    banner("G — the wire's own channel still reaches the list")
    before = app.query(f"{EXT}/selected_row")
    app.invoke(f"{EXT}/key", "ArrowDown")
    app.tick(16)
    assert_eq(
        app.query(f"{EXT}/selected_row") != before,
        True,
        "G: an agent driving with nothing focused still moves the selection",
    )


def body() -> None:
    with RpcSubprocess("hello-analyzer-shell", boot_grace=1.5) as app:
        dashboard(app)
    with RpcSubprocess("hello-packet-view", boot_grace=1.5) as app:
        capture(app)
    print(f"\n[demo] {len(CHECKS)} narrated check(s) beyond the assertions")


run_demo("R1698 a composite has a cursor inside it", body)
