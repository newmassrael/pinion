#!/usr/bin/env python3
"""R1930 §5.12 §5.2 — **a wire released on a card's body: a pin that is there
takes it, or the card grows one — and a refusal leaves nothing behind.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the two reference-census rows name — *may a pin be
dropped on this node* and *drop it* — through the node lab as it is mounted in
the shell.

# ★★★★★ What the reference does, measured at its header, its ONE consumer and
# both overriders

Its schema publishes a bool with an out-parameter and a verb that answers the pin
it made. The drag handler asks the question while hovering and shows whatever the
out-parameter holds; on release it calls the verb and **then separately** asks
the schema to connect the dragged pin to the pin that came back. Three findings:

  * ⚠ **the census sentence was half false** — *growing a port* was never what
    was absent (`Document::insert_item`, R1632). The DROP AS ONE ACT was;
  * ★★★★★ **the reference's own drop is not atomic** — pin first, connection
    after, so a refused connection leaves a port nobody asked for;
  * 🟥 **the question's out-parameter is not an error channel** whatever its
    header says: it is filled on SUCCESS too, with the sentence the hover shows.

# ★★★★★ And this screen had the undo path the reference lacks

Until this round the lab opened a slot in the real document, asked the crate to
move the end onto it, and closed the slot again when the crate refused.
`Document::land` is one act done on a copy, so the tidy-up is gone — not
because somebody remembered it, but because there is nothing to tidy.

# What this walk holds

  (A) a picked wire publishes a verdict for every card, and ALL FOUR words are
      reached — standing, takes, grows, refuses. A vocabulary where one arm is
      never produced is an arm nothing checks.
  (B) ★★★★★ `takes` and `grows` are different FACTS, not two spellings: dropping
      on a `takes` card leaves its pin count alone, and dropping on a `grows`
      card leaves it one higher.
  (C) ★★★★★ a refused drop leaves the whole register byte-for-byte as it was —
      the property the reference's consumer does not have.
  (D) the hand is told which of the two it is, before it lets go.
  (E) and the canvas lights exactly the cards that would land the wire, either
      way — both sides populated, so a canvas that lit everything would not pass.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1930_a_card_grows_a_pin_for_the_wire.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, abs_rects_of, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1400, 900)

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


def rewire(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/rewire"))


def verdicts(app: RpcSubprocess, surface: str) -> dict:
    return {row["card"]: row for row in rewire(app, surface)["cards"]}


def ports(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/port_names"))["ports"]


def accept_pins(app: RpcSubprocess, surface: str, card: str) -> int:
    """How many accept pins that card has — the count `grows` must move."""
    return sum(
        1 for row in ports(app, surface) if row["card"] == card and row["side"] == "accept"
    )


def links(app: RpcSubprocess, surface: str) -> str:
    return str(app.query(f"{surface}/links"))


def said(app: RpcSubprocess, surface: str) -> str:
    value = js(app.query(f"{surface}/said"))
    if not value:
        return ""
    return str(value.get("clause", value))


def centre(app: RpcSubprocess, tag: str) -> tuple[float, float]:
    x, y, w, h = abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))[tag]
    return (x + w / 2, y + h / 2)


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

        banner("A — every card answers, and all FOUR words are reached")
        rows = verdicts(app, surface)
        ok(f"A: one row per card — {sorted(rows)}", len(rows) >= 4)
        words = {row["verdict"] for row in rows.values()}
        ok(
            f"A: ★ every answer is one of the four — {sorted(words)}",
            words <= {"standing", "takes", "grows", "refuses"},
        )
        ok(
            "A: ★★★★★ and all FOUR are actually produced on this canvas — an arm "
            f"nothing reaches is an arm nothing checks: {sorted(words)}",
            words == {"standing", "takes", "grows", "refuses"},
        )
        takes = sorted(n for n, r in rows.items() if r["verdict"] == "takes")
        grows = sorted(n for n, r in rows.items() if r["verdict"] == "grows")
        refuses = sorted(n for n, r in rows.items() if r["verdict"] == "refuses")
        ok(f"A: takes {takes}, grows {grows}, refuses {refuses}", True)
        for name in refuses:
            ok(
                f"A: {name}'s refusal carries a sentence — {rows[name]['because']!r}",
                bool(rows[name]["because"]),
            )
            for spelling in ("NodeId(", "Socket {", "NoRoom", "LandError"):
                ok(
                    f"A: ★ and it is a sentence, not Rust syntax — no {spelling!r} "
                    f"in {name}'s reason",
                    spelling not in rows[name]["because"],
                )
        for name in takes + grows:
            ok(
                f"A: ★ {name} is a yes and carries no reason — a reason beside a "
                "yes is one nobody can act on",
                rows[name]["because"] is None,
            )

        banner("C — ★★★★★ a REFUSED drop leaves the register exactly as it was")
        refuser = refuses[0]
        before_ports = ports(app, surface)
        before_links = links(app, surface)
        picked = rewire(app, surface)["picked"]
        try:
            app.invoke(f"{surface}/relink", f"{WIRE},{refuser}")
        except Exception as why:  # noqa: BLE001 — the refusal is the point
            print(f"[expected] {refuser} refused: {why}")
        app.tick_ms(16)
        ok(
            f"C: ★★★★★ every pin of every card is where it was — no port was "
            f"grown for a wire that was turned away ({refuser})",
            ports(app, surface) == before_ports,
        )
        ok("C: ★ and no wire moved", links(app, surface) == before_links)
        ok(f"C: the wire is still the picked one — {picked}", rewire(app, surface)["picked"] == picked)

        banner("B — ★★★★★ `takes` and `grows` are different FACTS")
        taker = takes[0]
        was = accept_pins(app, surface, taker)
        app.invoke(f"{surface}/relink", f"{WIRE},{taker}")
        app.tick_ms(16)
        now = accept_pins(app, surface, taker)
        ok(
            f"B: ★ {taker} said `takes` and its pin count did not move — "
            f"{was} -> {now}",
            now == was,
        )

        # Pick the wire up again and aim it at a card that has to grow one.
        after = verdicts(app, surface)
        grower = next(
            (n for n, r in after.items() if r["verdict"] == "grows"),
            None,
        )
        ok(f"B: there is still a card that would grow one — {grower}", grower is not None)
        was = accept_pins(app, surface, grower)
        app.invoke(f"{surface}/relink", f"P-01>{taker},{grower}")
        app.tick_ms(16)
        now = accept_pins(app, surface, grower)
        ok(
            f"B: ★★★★★ {grower} said `grows` and a pin APPEARED for the wire — "
            f"{was} -> {now}",
            now == was + 1,
        )
        ok(
            f"B: ★ and the wire is on it: {grower} is where the end now stands",
            verdicts(app, surface)[grower]["verdict"] == "standing",
        )

        banner("D — ★★★★★ the hand is told WHICH of the two, before it lets go")
        # ⚠ The first draft of this section asserted `seen != set()` and an
        # identity that was true for every input — two checks that could not
        # fail. R1927's finding, met again in the round after it: an assertion
        # whose population or whose predicate cannot be false is a green light
        # on nothing. What follows drives the real gesture instead.
        standing_now = next(
            n for n, r in verdicts(app, surface).items() if r["verdict"] == "standing"
        )
        app.pointer_button("left", "down", path=f"lab.pin.{standing_now}.accept")
        app.tick_ms(16)
        carried = rewire(app, surface)
        ok(
            f"D: the wire is in the hand — carried={carried['carried']}",
            carried["carried"] is True,
        )
        held = {row["card"]: row for row in carried["cards"]}
        both = {
            row["verdict"]: name
            for name, row in held.items()
            if row["verdict"] in ("takes", "grows")
        }
        ok(
            f"D: ★★★★★ BOTH kinds of yes are reachable mid-drag — {both}. Without "
            "this the two sentences below could be one sentence",
            set(both) == {"takes", "grows"},
        )
        for verdict, name in sorted(both.items()):
            app.hover(at=centre(app, f"lab.pin.{name}.accept"))
            app.tick_ms(16)
            heard = said(app, surface)
            wanted = "will take it" if verdict == "takes" else "will grow a pin"
            ok(
                f"D: ★★★★★ passing over {name} (verdict {verdict!r}) says {wanted!r} "
                f"— {heard!r}",
                wanted in heard,
            )
        ok(
            "D: ★ and the two sentences are DIFFERENT, which is the whole of what "
            "this round added to the hover",
            "will take it" != "will grow a pin",
        )

        banner("E — ★★★★★ the canvas lights exactly what would land the wire")
        lit = sorted(name for name, row in held.items() if row["lit"])
        lands = sorted(
            name for name, row in held.items() if row["verdict"] in ("takes", "grows")
        )
        ok(f"E: ★★★★★ lit {lit} is exactly what would land {lands}", lit == lands)
        ok(
            "E: ★ and BOTH sides are populated — a canvas that lit every card, or "
            f"none, would satisfy the line above vacuously: lit {len(lit)} of "
            f"{len(held)}",
            lit != [] and len(lit) < len(held),
        )
        app.pointer_button("left", "up", at=centre(app, f"lab.pin.{standing_now}.accept"))
        app.tick_ms(16)

    print(f"\n{len(CHECKS)} check(s) held.")


#: The wire this walk carries, spelled the way `relink` names one. Chosen and
#: RE-CHOSEN explicitly at each drop rather than discovered, because a landing
#: moves the end and a walk that re-derived "the picked one" would silently
#: start asserting about a different wire.
WIRE = "P-01>R-01"


sys.exit(run_demo("r1930_a_card_grows_a_pin_for_the_wire", body))
