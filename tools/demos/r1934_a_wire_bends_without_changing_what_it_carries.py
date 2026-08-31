#!/usr/bin/env python3
"""R1934 §5.2 §5.11 — **a wire bends, and what it carries does not change.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the two reference-census rows name — the DCC's
`add_reroute` and the engine's `ShouldDrawNodeAsControlPointOnly` — through the
node lab as it is mounted in the shell.

# ★★★★★ The two rows are two mechanisms, and the carried hint was right

One is a **verb** that makes nodes; the other is a **question** an editor asks a
node. Measured apart:

  * **the DCC's is a gesture across the canvas.** Its operator takes the
    polyline the pointer drew, intersects it with every drawable link, and the
    behaviour nobody would guess from the name is that it groups by SOURCE
    SOCKET — cutting a fan-out of three wires leaving one pin makes ONE reroute
    that all three then leave from. Its own comment says so: "deduplicating new
    reroutes per output socket is useful because it allows reusing reroutes for
    connected intersections". It also keeps the cut links and re-points them,
    mutes the new feed exactly when every cut link was muted, and lands the
    reroute at the average of its crossings.
  * **the engine's is named for drawing and draws nothing.** Across all seven
    of its call sites: three pick which END of the point a drag should take, one
    spreads a hover along the chain, one keeps the point's pins out of node
    alignment, and one asserts it as a precondition. So the capability is "a
    wire passes through here, and these are its two ends" — the name is the one
    thing about it that is wrong.

# What the lab publishes for it

`insert_reroute` names the SOURCE PIN rather than a drawn line, because the
geometry belongs to the screen and the source socket is the operator's own unit
of work. `passing` is the register: which things on the canvas a wire runs
through, their two ends, and what each bend is carrying — a bend nothing has
decided is a real state and a client drawing one has to tell it from a decided
one.

# What this walk holds

  (A) the journey reaches the node lab, so what follows is about the ASSEMBLED
      tool, and NOTHING on the opening canvas is a point on a wire.
  (B) ★★★★★ three wires leave one pin and ONE bend appears — the deduplication,
      which is the behaviour a per-wire implementation would fail.
  (C) ★★★★★ the register reports the bend with BOTH ends, and says it carries a
      value — so a client can tell it from an undecided one.
  (D) ★★★★★ the three wires KEPT THEIR IDENTITY: same ids, now leaving the bend,
      and the source pin feeds exactly one wire.
  (E) ★★★★★ AND THE TOOL STILL SAYS THE SAME THING ABOUT ITSELF — the launch
      verdict is what it was. Bending a wire is not an edit to the pipeline.
  (F) the refusal: a bend goes on the wires LEAVING a pin, and naming an
      `accept` is turned away with the reason rather than doing nothing.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1934_a_wire_bends_without_changing_what_it_carries.py
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

#: The card three wires leave, named rather than discovered: a walk that hunted
#: for a fan-out would quietly assert about whichever one it found.
FAN_OUT = "R-01"

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


def links(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/links"))


def leaving(rows: list[dict], card: str) -> list[dict]:
    return [row for row in rows if row["from"] == card]


def passing(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/passing"))["through"]


def verdict(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/verdict"))


def bend(app: RpcSubprocess, surface: str, card: str, address: str):
    """Answer the refusal's sentence, or None when the bend went in."""
    try:
        app.invoke(f"{surface}/insert_reroute", f"{card},{address}")
        return None
    except Exception as why:  # noqa: BLE001 — the refusal IS the measurement
        return str(why)


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("A — nothing on the opening canvas is a point on a wire")
        ok(
            f"A: the register answers, and it is empty — {passing(app, surface)}",
            passing(app, surface) == [],
        )
        before_links = links(app, surface)
        fan = leaving(before_links, FAN_OUT)
        ok(
            f"A: ★ {FAN_OUT} fans out to {[row['to'] for row in fan]} — a "
            "deduplication that had one wire to work on could not be told from "
            "one that had none",
            len(fan) >= 3,
        )
        was = verdict(app, surface)
        ok(f"A: and the tool says what it says about itself — {was}", "sentence" in was)

        banner("B — ★★★★★ three wires leave one pin and ONE bend appears")
        said = bend(app, surface, FAN_OUT, "dial")
        ok(f"B: the bend went in — {said!r}", said is None)
        app.tick_ms(16)
        through = passing(app, surface)
        ok(
            f"B: ★★★★★ ONE point on a wire, for {len(fan)} cut wires — {through}",
            len(through) == 1,
        )

        banner("C — ★★★★★ the register names both ends, and what it carries")
        point = through[0]
        ok(f"C: ★ it names the end a wire arrives at — {point}", point["in"] == 0)
        ok(f"C: ★ and the end it leaves by — {point}", point["out"] == 0)
        ok(
            f"C: ★★★★★ and it says it carries a VALUE, not that it is undecided "
            f"— {point['carries']!r}. A bend nothing had decided would say "
            "`undecided` here, and a client drawing the two the same way would "
            "be drawing a lie",
            point["carries"] == "value",
        )

        banner("D — ★★★★★ the wires kept their identity")
        after_links = links(app, surface)
        ok(
            f"D: ★ the source pin now feeds exactly one wire — "
            f"{[row['to'] for row in leaving(after_links, FAN_OUT)]}",
            len(leaving(after_links, FAN_OUT)) == 1,
        )
        bent = point["card"]
        from_bend = leaving(after_links, bent)
        ok(
            f"D: ★★★★★ and every reader now hangs off the bend — "
            f"{[row['to'] for row in from_bend]}",
            len(from_bend) == len(fan),
        )
        kept = {row["id"] for row in fan} & {row["id"] for row in from_bend}
        ok(
            f"D: ★★★★★ the SAME LINK IDS, re-pointed rather than remade — {sorted(kept)}. "
            "A verb that deleted and rebuilt them would leave an undo stack "
            "holding wires that no longer exist",
            kept == {row["id"] for row in fan},
        )
        ok(
            f"D: ★ and every reader the wires had is still reached — "
            f"{sorted(row['to'] for row in from_bend)}",
            sorted(row["to"] for row in from_bend) == sorted(row["to"] for row in fan),
        )

        banner("E — ★★★★★ and the tool still says the same thing about itself")
        now = verdict(app, surface)
        ok(
            f"E: ★★★★★ the launch verdict is what it was — {now}. Bending a wire "
            "is not an edit to the pipeline, and a bend that changed this would "
            "have changed what the graph MEANS",
            now == was,
        )

        banner("F — the refusal says which way a bend goes")
        why = bend(app, surface, FAN_OUT, "accept")
        ok(f"F: ★★★★★ an `accept` is REFUSED — {why!r}", why is not None)
        ok(
            f"F: ★ and the refusal says why, in the screen's own words — {why!r}",
            "LEAVING" in (why or ""),
        )
        ok(
            f"F: ★ and it changed nothing — still one point on a wire — "
            f"{passing(app, surface)}",
            len(passing(app, surface)) == 1,
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1934_a_wire_bends_without_changing_what_it_carries", body))
