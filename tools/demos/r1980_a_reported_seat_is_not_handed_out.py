#!/usr/bin/env python3
"""R1980 §5.2 §5.11 — **a seat a report is sitting on is not handed to somebody
else**, and the card's kind is what says where an arriving end berths.

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the node lab's canvas gesture — picking a wire and asking, of every
card, what releasing it there would do — on the opening graph, which holds one
**reported** connection nobody drew.

# ★★★★★ Two debts, one derivation, and the premise was DRIVEN before it was fixed

`debt-what-lands-on-a-socket-is-spelled-by-four-readers-and-one-counts-half`
was registered `blocked_by: unmeasured`, so R1980's first act was to run it
rather than to prescribe for it. On the opening graph, before anything changed:

    before   port 0: links=["link#4"]  obs=[]
             port 1: links=[]          obs=["obs<-P-01"]
    may_land(the store's wire, Input, P-02)  ->  Takes(port 1)
    after    port 1: links=["link#3"]  obs=["obs<-P-01"]

An unrelated wire took the seat the report was sitting on, and the screen then
wrote its own address over it. The premise held. Driven again after the repair,
the same call answers `Grows(port 2)` and the report keeps its seat.

⚠ The measurement also CORRECTED the debt: it listed four readers of *is
anything on this socket* and there are three. `free_endpoints_in` asks a
different question — *which addresses has this dialler taken on that card* —
whose population is a pair, and it was left alone with that written beside it.

# ★★★★★ And a kind now says WHERE an end berths (`dcc node::insert_link`)

The reference hands a node type the link that was dropped on it and lets it
intervene; measured across all 41 sites that install it, the 25 that do anything grow
a port and move the end onto it. This crate already refused pairs (R1885) and
grew ports (R1930) — what was absent was **which seat**, and the census pin's
covering sentence was false in both its clauses. `NodeKind::berth` is that
choice, as a policy the document enforces rather than a hook that edits the
graph and answers a bool.

# What this walk holds

  (A) the journey reaches the node lab, so what follows is about the assembled
      tool.
  (B) the opening graph carries a reported connection, on a card the screen
      names, and it is NOT in the drawing.
  (C) ★★★★★ carrying an unrelated wire over that card, the screen says a pin
      will GROW — the reported seat is not offered to it.
  (D) ★ and that is not "always grow": a card whose seat is genuinely empty
      still says the pin there TAKES it, so (C) is about the report.
  (E) ★★★★★ releasing it really does leave the report where it was, with its
      address intact.
  (F) ⚠ but ADOPTING the report puts a drawn link on exactly that seat — a
      person naming it is the whole of what adoption means, and erasing that
      distinction would break it.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1980_a_reported_seat_is_not_handed_out.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def verdicts(app: RpcSubprocess, surface: str) -> dict[str, dict]:
    """What the screen would do with the picked wire on each card, by card."""
    wire = js(app.query(f"{surface}/rewire"))
    return {row["card"]: row for row in wire["cards"]}


def links_of(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/links"))


def reports_of(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/observed"))


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "A: the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("B — the opening graph carries a connection nobody drew")
        reports = reports_of(app, surface)
        ok(
            f"B: ★ exactly one reported connection, and the screen names both "
            f"ends and the address it took — {reports}",
            len(reports) == 1 and reports[0]["endpoint"],
        )
        held = reports[0]["to"]
        reporter = reports[0]["from"]
        drawn = links_of(app, surface)
        ok(
            f"B: ★★★★★ and it is NOT in the drawing — {reporter} -> {held} is a "
            f"claim about the world, which is why a seat it sits on is a seat "
            f"nobody drew on",
            not any(row["from"] == reporter and row["to"] == held for row in drawn),
        )

        banner("C — ★★★★★ an unrelated wire is not offered the reported seat")
        # A wire whose consuming end is somewhere else entirely: it has nothing
        # to do with the report, which is exactly the point.
        stranger = next(
            row for row in drawn if row["to"] != held and row["from"] != reporter
        )
        app.invoke(f"{surface}/select_link", str(stranger["id"]))
        app.tick_ms(16)
        said = verdicts(app, surface)
        ok(
            f"C: ★ the screen answers for every card on the canvas, so a yes is "
            f"read rather than inferred from an absence — {sorted(said)}",
            len(said) > 1 and held in said,
        )
        ok(
            f"C: ★★★★★ over the card the report sits on, the wire says a pin "
            f"will GROW — until R1980 this said 'takes', and what it would have "
            f"taken was the reported seat — {said[held]}",
            said[held]["verdict"] == "grows",
        )

        banner("D — ★ and that is about the REPORT, not about always growing")
        # Free a seat that nothing reported: unlink a card whose only inbound
        # wire is a drawn one. The screen closes the slot it opened, and the run
        # keeps the floor its kind declares — so the card is left with an empty
        # seat that no report is on.
        # ⚠ It has to be a card with EXACTLY ONE inbound wire: the first draft
        # picked the router, which three wires reach, so unlinking one left the
        # seat as occupied as before and the check failed for a reason that had
        # nothing to do with what it was asking.
        inbound = {}
        for row in drawn:
            inbound[row["to"]] = inbound.get(row["to"], 0) + 1
        elsewhere = next(
            row
            for row in drawn
            if row["to"] not in (held, stranger["to"]) and inbound[row["to"]] == 1
        )
        emptied = elsewhere["to"]
        app.invoke(f"{surface}/delete_link", str(elsewhere["id"]))
        app.tick_ms(16)
        ok(
            f"D: ★ {emptied} now has nothing landing on it — neither drawn nor "
            f"reported",
            not any(row["to"] == emptied for row in links_of(app, surface))
            and not any(row["to"] == emptied for row in reports_of(app, surface)),
        )
        app.invoke(f"{surface}/select_link", str(stranger["id"]))
        app.tick_ms(16)
        said = verdicts(app, surface)
        ok(
            f"D: ★★★★★ and THERE the pin that is already on it TAKES the wire — "
            f"so (C) is the report being counted, not a card that always grows "
            f"— {emptied}: {said[emptied]}",
            said[emptied]["verdict"] == "takes",
        )
        ok(
            f"D: ★ while the reported card still says grow, in the same reading "
            f"— {held}: {said[held]}",
            said[held]["verdict"] == "grows",
        )

        banner("E — ★★★★★ releasing it leaves the report where it was")
        was = reports_of(app, surface)
        app.invoke(f"{surface}/relink", f"{stranger['id']},{held}")
        app.tick_ms(16)
        now = reports_of(app, surface)
        ok(
            f"E: ★★★★★ the reported connection is untouched — same ends, same "
            f"address, same layer — {was} -> {now}",
            now == was,
        )
        ok(
            f"E: ★ and the wire really did move, so the check above is about a "
            f"landing that HAPPENED — {links_of(app, surface)}",
            any(
                row["id"] == stranger["id"] and row["to"] == held
                for row in links_of(app, surface)
            ),
        )

        banner("F — ⚠ adopting the report DOES take that seat")
        said = app.invoke(f"{surface}/adopt", f"{reporter},{held}")
        app.tick_ms(16)
        ok(
            f"F: ⚠★★★★★ a person naming the reported connection puts a drawn "
            f"link on it — the automatic search must not choose that seat and "
            f"adoption must — {said!r}",
            any(
                row["from"] == reporter and row["to"] == held
                for row in links_of(app, surface)
            ),
        )
        ok(
            f"F: ★ and the report is still there, now matched by a drawing — "
            f"{reports_of(app, surface)}",
            any(
                row["from"] == reporter and row["to"] == held
                for row in reports_of(app, surface)
            ),
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1980 a reported seat is not handed out", body)
