#!/usr/bin/env python3
"""R1695 §5.38 §5.39 §5.40 §2 #2 #7 — **arriving is not highlighting.**

The analysis tool's navigation rail is the first thing a person touches and the
last thing anybody checked. Driven through the §5.35 router and measured on the
running application the day this round opened:

* pressing **four of the seven seats** — Stream, Decode, Catalog, Settings —
  moved the string the rail highlights itself from and left the window at
  **193 tagged regions before and 193 after**. The screen said *Stream* and
  showed the dashboard;
* on the sibling screen every seat, **including the one that screen already
  was**, answered with a message saying the destination is not this screen, and
  the two seats declared unavailable with a stated reason got that message
  rather than their reason;
* the two screens' rosters share **two keys out of seven**. One tool, two lists
  of what the tool contains.

The screen's own demo was green through all of it, and the reason is exact: it
asserted that a press **moved the state**, which is what a rail that highlights
a seat and shows nothing new does. *Moved* is not *arrived*, and the difference
is a rectangle.

So this script is the integration test for the rail as the specification
describes it. It drives every destination through the router the way a mouse
arrives, and asserts against the painted scene:

* **A** — the roster the screen publishes is the specification's rail, standing
  for standing, in both directions.
* **B** — every open destination arrives and paints its own page; every closed
  one refuses and names the reason its seat is painted with; the pages are
  pairwise distinct. (`assert_every_destination_arrives`, the law written once.)
* **C** — the region says which destination is showing, to a reader as well as
  to the wire. Measured on the reference toolkit at 6.11.1 by building and
  running a probe: its paged container reports a layered pane whose accessible
  **value is empty**, so no client can ask that at all.
* **D** — the Settings destination, reproduced from the reference: four
  switches in two groups, two affordances booked for a later release, and a
  two-way appearance segment. Every one of them pressed where it is painted.
* **E** — the census is judged **at both destinations**. Every previous run of
  it judged the opening screen and nothing else, which is why a page could be
  added to this screen and be seen by no gate at all.
* **F** — a page keeps its state across a departure and a return.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1695_the_rail_takes_you_there.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import closed_keys, closed_kinds, owed_keys  # noqa: E402
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    assert_every_destination_arrives,
    assert_router_press_moves,
    bring_into_view,
    run_demo,
)

EXAMPLE = "hello-analyzer-shell"
EXT = "/external"
CHECKS: list[str] = []


closed_rail_keys = closed_keys


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


def nodes_by_tag(app: RpcSubprocess) -> dict:
    return {n["tag"]: n for n in app.request("scene/access").result["nodes"]}


def refused(app: RpcSubprocess, path: str, value) -> str:
    try:
        app.intervene(f"{EXT}/{path}", value)
    except Exception as why:  # noqa: BLE001 - any refusal shape is fine here
        return str(why)
    raise AssertionError(f"a write of {value!r} to {path} was expected to refuse")


def go(app: RpcSubprocess, key: str) -> None:
    app.intervene(f"{EXT}/nav", key)
    app.tick_ms(16)
    assert_eq(q(app, "nav"), key, f"the journey reached {key}")


def body() -> None:  # noqa: PLR0915 - one narrative, read top to bottom
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as app:
        spec = q(app, "spec")

        # ── (A) the roster IS the specification's rail ─────────────────────
        banner("A — the roster the screen navigates by is the rail it declares")
        roster = q(app, "destinations")
        ok("the screen publishes a roster", isinstance(roster, dict))
        rows = {row["key"]: row for row in roster["destinations"]}
        declared = {seat["key"]: seat for seat in spec["rail"]}
        assert_eq(sorted(rows), sorted(declared), "A: the roster is the rail")
        assert_eq(roster["at"], spec["rail_active"], "A: it opens where it says")
        for key, seat in declared.items():
            row = rows[key]
            assert_eq(row["open"], seat["open"], f"A: {key} standing")
            assert_eq(row["title"], seat["title"], f"A: {key} title")
            if seat["reserved_for"]:
                assert_eq(row["kind"], "reserved", f"A: {key} is booked")
                assert_eq(row["detail"], seat["reserved_for"], f"A: {key} booking")
                assert_eq(row["recourse"], "await_release", f"A: {key} recourse")
            elif not seat["open"]:
                # ★★★★★ R1730 — this asserted `elsewhere` and had done since
                # R1695, when every non-reserved shut seat WAS built on another
                # surface. R1728 replaced that kind with `unbuilt` for the
                # sections the reference has and this build has not, and R1729
                # mounted the last real `elsewhere` — so this demo had been RED
                # for two rounds and nothing ran it. That is the finding, and
                # the repair is not a new literal: the kind comes from
                # `docs/analyzer-rail-spec.json` and the recourse from the
                # framework's own derivation, so the next kind to appear here
                # moves this expectation by itself.
                assert_eq(
                    row["kind"],
                    "unbuilt" if key in owed_keys() else "elsewhere",
                    f"A: {key} says WHICH kind of shut it is",
                )
                assert_eq(
                    row["recourse"],
                    "await_release" if key in owed_keys() else "open_elsewhere",
                    f"A: {key} recourse",
                )
                ok(f"A: {key} names what would open it", bool(row["detail"]))
        opens = [k for k, r in rows.items() if r["open"]]
        closed = [k for k, r in rows.items() if not r["open"]]
        # ★★★★★ R1724 — Catalog's page is now the node graph lab, mounted whole
        # (`pinion_screen::Mount<NodeLabView>`), so what this application hosts
        # grew by a destination without a line of that screen changing.
        # `assert_every_destination_arrives` below drives it like any other seat
        # — which is the point: a mounted screen is a destination, not a special
        # case.
        # ★★★★★ R1730 — and the two counts are DERIVED. Written out they were
        # 3 and 4, both stale since R1728, and nothing ran this demo to say so.
        assert_eq(
            sorted(closed),
            closed_rail_keys(),
            "A: what it declares and cannot take you to is exactly what its "
            "specification says is shut",
        )
        assert_eq(
            len(opens),
            len(rows) - len(closed),
            "A: and everything else is a destination this application hosts",
        )
        kinds = sorted({rows[k]["kind"] for k in closed})
        # ★ R1953 — derived from WHICH seats are shut, not from a set with
        # `reserved` written into it. That spelling was true while a reserved
        # seat was always shut, and R1947/R1948 opened both of them: the rail
        # then had no shut seat at all and this asked for a word nothing could
        # carry.
        assert_eq(
            kinds,
            closed_kinds(),
            "A: the ways to be closed, derived from the specification",
        )
        print(f"[demo] roster: {len(rows)} destination(s), {len(opens)} open, {kinds}")

        # ── (B) the law, driven through the router ─────────────────────────
        banner("B — every open destination is a place you arrive at")
        assert_every_destination_arrives(
            app,
            roster_path=f"{EXT}/destinations",
            seat=lambda key: f"shell.rail.{key}",
            region="shell.canvas",
        )
        ok("B: every destination was driven through the router", True)

        # Pressing the seat you are already on is not a refusal and not a move.
        go(app, "dashboard")
        before = abs_rects_of(app.snapshot(source="paint"))
        seat = before["shell.rail.dashboard"]
        app.request(
            "scene/click",
            {"button": "left", "at": {"x": seat[0] + seat[2] // 2, "y": seat[1] + seat[3] // 2}},
        )
        app.tick_ms(16)
        assert_eq(q(app, "nav"), "dashboard", "B: already here stays here")
        assert_eq(
            frozenset(abs_rects_of(app.snapshot(source="paint"))),
            frozenset(before),
            "B: and paints the same page",
        )
        # ★★ R1719 — asked as "what KIND of thing did the screen say", not as
        # "does the wording contain the word already". The tone is what this
        # line was always about, and a substring probe would have passed on a
        # sentence about something else entirely that happened to say "already".
        ok(
            "B: the toast says so rather than refusing",
            q(app, "said")["tone"] == "unchanged",
        )

        # ── (C) the region says where you are ──────────────────────────────
        banner("C — the page region names its destination, for a reader too")
        for key, title in (("dashboard", "Dashboard"), ("settings", "Settings")):
            go(app, key)
            region = nodes_by_tag(app)["shell.canvas"]
            assert_eq(region["role"], "region", f"C: {key} region role")
            assert_eq(region["name"], title, f"C: {key} region name")
            rail_seat = nodes_by_tag(app)[f"shell.rail.{key}"]
            assert_eq(rail_seat["current"], "page", f"C: {key} seat is current")
            assert_eq(rail_seat["name"], title, "C: the seat and the region agree")
        # And the seats you cannot reach say why, in the tree.
        tree = nodes_by_tag(app)
        for key in closed:
            node = tree[f"shell.rail.{key}"]
            un = node.get("unavailable")
            ok(f"C: the {key} seat carries its reason", isinstance(un, dict))
            assert_eq(un["kind"], rows[key]["kind"], f"C: {key} kind in the tree")
            assert_eq(un["detail"], rows[key]["detail"], f"C: {key} detail in the tree")
            ok(f"C: the {key} seat is announced unavailable", node["state"]["disabled"])

        # ── (D) the Settings destination, as the reference states it ───────
        banner("D — the settings page: four switches, two bookings, one segment")
        go(app, "settings")
        assert_eq(len(spec["options"]), 4, "D: four switches")
        assert_eq(len(spec["key_rows"]), 2, "D: two booked affordances")
        assert_eq(len(spec["option_groups"]), 4, "D: four groups")
        assert_eq(list(spec["themes"]), ["Dark", "Light"], "D: two themes")
        opening = {row["key"]: row["on"] for row in q(app, "options")}
        assert_eq(
            opening,
            {row["key"]: row["opens"] for row in spec["options"]},
            "D: the page opens on the specified values",
        )
        # ★ They alternate, which is what stops a check that reads the wrong
        # switch from passing anyway.
        ok("D: the opening values are not all the same", len(set(opening.values())) == 2)

        for option in spec["options"]:
            key = option["key"]
            was = {r["key"]: r["on"] for r in q(app, "options")}[key]
            assert_router_press_moves(
                app,
                f"shell.settings.option.{key}",
                lambda: {r["key"]: r["on"] for r in q(app, "options")},
                f"D: the {key} switch",
            )
            now = {r["key"]: r["on"] for r in q(app, "options")}[key]
            assert_eq(now, not was, f"D: {key} flipped")
            # The knob moved with it — the part a hand-drawn track does not do.
            ok(f"D: {key} is a switch to a reader", True)

        # ★★★★★ R1867 — the segments are pressed where a READER finds them, and
        # this loop used to assume where that was.
        #
        # It read the rectangle straight out of the paint snapshot, which is a
        # claim that the control is above the fold — true until R1864 gave the
        # window a status band and the settings page's last group moved below
        # the viewport. The demo then died `KeyError('shell.settings.theme.0')`
        # and stayed red for two rounds, and **the screen was not wrong**:
        # `scene/scroll_reach` reported the segments `scrollable`, `lost: 0`,
        # with the offset that shows them. A fold position is not what this
        # section is about; pressing the segment is.
        for n, name in enumerate(spec["themes"]):
            x, y, w, h = bring_into_view(app, f"shell.settings.theme.{n}")
            app.request(
                "scene/click", {"button": "left", "at": {"x": x + w // 2, "y": y + h // 2}}
            )
            app.tick_ms(16)
            assert_eq(q(app, "theme"), name.lower(), f"D: the {name} segment")
        # The two booked rows refuse, and say what they are booked under.
        tree = nodes_by_tag(app)
        for row in spec["key_rows"]:
            node = tree[f"shell.settings.key.{row['key']}"]
            un = node.get("unavailable")
            ok(f"D: {row['key']} carries its booking", isinstance(un, dict))
            assert_eq(un["kind"], "reserved", f"D: {row['key']} kind")
            assert_eq(un["detail"], row["reserved_for"], f"D: {row['key']} booking")

        # ── (E) the census, judged at BOTH destinations ────────────────────
        banner("E — every destination's regions are judged, not only the opening one")
        seen = {}
        for key in opens:
            go(app, key)
            voice = app.request("scene/voice").result
            conform = app.request("scene/conform").result
            counts = voice["counts"]
            assert_eq(counts["unvoiced"], 0, f"E: nothing undecided at {key}")
            for fault in ("mumbled", "hollow", "dangling", "ghost"):
                assert_eq(counts[fault], 0, f"E: no {fault} region at {key}")
            assert_eq(conform["counts"]["empty"], 0, f"E: no empty collection at {key}")
            assert_eq(conform["counts"]["stray"], 0, f"E: no displaced member at {key}")
            # `judged` sits beside `counts`, not inside it. Read wrong on the
            # first draft and it silently reported zero for both destinations —
            # the failure mode a `.get(..., 0)` produces and the reason this
            # asserts the number is non-zero rather than only printing it.
            judged = conform["judged"]
            ok(f"E: {key} has structure to judge", judged > 0)
            seen[key] = (voice["total"], judged)
            print(
                f"[demo] {key}: {voice['total']} region(s), "
                f"{counts['announced']} announced, {counts['silent']} quiet, "
                f"{judged} structurally judged"
            )
        ok(
            "E: the two destinations are different screens to a reader too",
            seen["dashboard"][0] != seen["settings"][0],
        )

        # ── (F) a page keeps its state across a departure ──────────────────
        banner("F — leaving a page and coming back does not reset it")
        go(app, "settings")
        flipped = {r["key"]: r["on"] for r in q(app, "options")}
        go(app, "dashboard")
        go(app, "settings")
        assert_eq(
            {r["key"]: r["on"] for r in q(app, "options")},
            flipped,
            "F: the switches survived the round trip",
        )
        # ★ The discriminating half: they are NOT the opening values, so this
        # is not satisfied by a page that resets to a state that happens to
        # match. Every switch was flipped in (D).
        ok("F: and they are not the opening values", flipped != opening)

        # ── (F2) ★★ R1696 — the screen has a keyboard ─────────────────────
        banner("F2 — the Tab ring is the one the specification declares")
        ring = spec["focus_ring"]
        ok("F2: the specification declares a ring at all", len(ring) > 0)
        for key in opens:
            go(app, key)
            want = [row["tag"] for row in ring if row["at"] in ("*", key)]
            assert want, f"F2: {key} declares no stop"
            # Walk it the way a Tab key does and let it close. There is no
            # `focus/clear` on the wire, so the walk starts wherever the last
            # section left it and the ring is read by ROTATING to the first
            # declared stop — which is also the stronger check: a ring is a
            # cycle, and asserting the cycle rather than a prefix means the
            # order holds from any entry point.
            seen: list[str] = []
            # ★★★★★ R1724 — the bound is on the RING, and the ring is no longer
            # only this screen's. Catalog's page is the node graph lab, mounted
            # whole, so its stops are in the cycle too — and `want` counts only
            # the composites this screen's table declares. Sized from `want`,
            # the walk stopped before it came back round and reported the first
            # declared stop as missing from a ring it had simply not finished.
            # The loop already ends when the cycle closes; this is the runaway
            # guard, so it is sized for a window that can hold another screen.
            for _ in range(400):
                stop = app.request("focus/next").result.get("focused")
                assert stop is not None, (
                    f"F2: at {key} the ring ran out — the screen announces "
                    f"operable roles and a keyboard reaches nothing"
                )
                if stop in seen:
                    seen = seen[seen.index(stop) :]
                    break
                seen.append(stop)
            composites = [tag for tag in seen if tag in {row["tag"] for row in ring}]
            rotate = composites.index(want[0])
            composites = composites[rotate:] + composites[:rotate]
            assert_eq(composites, want, f"F2: the ring at {key} is the declared one")
            # Each stop says what it holds — the part a tag cannot carry.
            for row in (r for r in ring if r["at"] in ("*", key)):
                ok(f"F2: {row['tag']} says what it holds", len(row["holds"]) > 10)
            # And the reader is told where they are.
            app.request("focus/set", {"tag": want[0]})
            app.tick_ms(16)
            focus = app.request("scene/access").result.get("focus") or {}
            assert_eq(focus.get("tag"), want[0], f"F2: AT focus rests on {want[0]}")
            assert_eq(focus.get("resolved"), "tag", f"F2: and it resolved to a node")
        go(app, "settings")
        # ★ The settings page's own controls are stops in their own right,
        # because a switch is not inside a composite — and they come from the
        # catalogue widget rather than from the ring table.
        reached: set[str] = set()
        for _ in range(24):
            stop = app.request("focus/next").result.get("focused")
            if stop is None or stop in reached:
                break
            reached.add(stop)
        for option in spec["options"]:
            ok(
                f"F2: the {option['key']} switch is a stop of its own",
                f"shell.settings.option.{option['key']}" in reached,
            )
        # ★ NEGATIVE CONTROL — a booked affordance is NOT a stop. Without this
        # the walk above is satisfied by stamping every node focusable.
        for row in spec["key_rows"]:
            ok(
                f"F2: ★ the booked {row['key']} button is not a stop",
                f"shell.settings.key.{row['key']}" not in reached,
            )
        go(app, "dashboard")

        # ── The closed set, on the wire ────────────────────────────────────
        banner("G — the wire refuses what the rail refuses, in the same words")
        assert "is not a rail section" in refused(app, "nav", "nowhere"), "G: closed set"
        for key in closed:
            said = refused(app, "nav", key)
            ok(f"G: {key} refuses on the wire", rows[key]["detail"] in said)
        assert_eq(q(app, "nav"), "dashboard", "G: no refusal moved the journey")

        print(f"\n[demo] {len(CHECKS)} narrated check(s) beyond the assertions")


run_demo("R1695 the rail takes you there", body)
