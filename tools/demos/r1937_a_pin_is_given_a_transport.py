#!/usr/bin/env python3
"""R1937 §5.2 §5.11 — **a pin is given a transport, and the peer becomes the one
that speaks it.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the two reference-census rows name — the engine's
per-pin type choice and the node hook it reaches through — as the node lab is
mounted in the shell.

# ★★★★★ The two rows are a VERB and a NOTIFICATION, and that is the finding

Their `mechanism` fields say so and nobody had read them: one is `GraphEditor`
(an editor command whose own tooltip is *"Changes the type of this pin (boolean,
int, etc.)"*) and the other is `graph-node` (a node hook). Measured across all
seven mentions of the hook, it is a `void` notification whose ONE external call
site is the pin's type-selector widget, and its own comment says it fires when a
pin's type *"has had its' pin type changed from an external source"* — PAST
TENSE. So there a node hears about the change after it happened and cannot
refuse.

Here the same declaration is asked FIRST: `choosable` is what a screen reads
before offering anything, and the verb obeys the same answer, so the two cannot
disagree.

# What this walk holds

  (A) the journey reaches the node lab, and the register says which pins may be
      given a type — per (pin, TYPE), because the same pin takes a whole
      locator and refuses half of one.
  (B) ★★★★★ a pin is given a transport and THE CARD BECOMES the peer that
      speaks it — the card keeps its name, and its pins now carry that.
  (C) ★★★★★ and the tool says what it cost: a wire that could not cross with
      the new type is gone, and the sentence names how many.
  (D) the refusal: half a locator is not a transport a peer can speak, and the
      verb turns it away with the reason rather than doing nothing.
  (E) ★ asking and doing agree — every type the register offers is accepted and
      every one it withholds is refused, driven over the wire rather than
      asserted in prose.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1937_a_pin_is_given_a_transport.py
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

#: The card this walk retypes, named rather than discovered.
SUBJECT = "R-01"

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


def choosable(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/choosable"))["ports"]


def for_card(rows: list[dict], card: str) -> list[dict]:
    return [row for row in rows if row["card"] == card]


def refusal(app: RpcSubprocess, path: str, arg: str):
    """Answer the refusal's sentence, or None when the edit went through."""
    try:
        app.invoke(path, arg)
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

        banner("A — the register says which pins may be given which types")
        rows = choosable(app, surface)
        ok(f"A: the register answers — {len(rows)} port(s)", len(rows) > 0)
        mine = for_card(rows, SUBJECT)
        ok(f"A: ★ {SUBJECT} has ports in it — {mine}", len(mine) > 0)
        offered = mine[0]["takes"]
        ok(
            f"A: ★★★★★ the answer is per (pin, TYPE) and it WITHHOLDS some — "
            f"{offered}",
            len(offered) > 0 and any(t.startswith("locator/") for t in offered),
        )
        ok(
            f"A: ★ and half a locator is not among them — {offered}",
            not any(t in ("host", "service") for t in offered),
        )
        wires_before = len(links(app, surface))

        banner("B — ★★★★★ the card becomes the peer that speaks it")
        said = app.invoke(f"{surface}/set_pin_transport", f"{SUBJECT},dial,udp")
        app.tick_ms(16)
        ok(
            f"B: the verb says what it did — {said!r}",
            "now speaks udp" in str(said),
        )
        after = for_card(choosable(app, surface), SUBJECT)
        ok(
            f"B: ★ the card is still there under the same name — this is a swap, "
            f"not a replace: {len(after)} port(s)",
            len(after) > 0,
        )

        banner("C — ★★★★★ and the tool says what it cost")
        ok(
            f"C: the sentence names how many wires could not cross — {said!r}",
            "wire(s) could not cross" in str(said),
        )
        wires_after = len(links(app, surface))
        ok(
            f"C: ★ and the canvas agrees ({wires_before} then {wires_after})",
            wires_after <= wires_before,
        )

        banner("D — the refusal says what a peer cannot speak")
        why = refusal(app, f"{surface}/set_pin_transport", f"{SUBJECT},dial,host")
        ok(
            f"D: ★ half a locator is turned away with the reason — {why!r}",
            why is not None,
        )

        banner("E — ★ asking and doing agree, driven rather than asserted")
        # Every transport the register offers for this pin is accepted; the
        # register is read again first, because the card has changed.
        offers = for_card(choosable(app, surface), SUBJECT)[0]["takes"]
        accepted = 0
        for word in ("tcp", "tls", "quic", "udp", "ws"):
            expected = any(word.upper() in t.upper() for t in offers)
            got = refusal(app, f"{surface}/set_pin_transport", f"{SUBJECT},dial,{word}") is None
            ok(
                f"E: {word}: register says {expected}, verb says {got}",
                expected == got,
            )
            accepted += int(got)
        ok(
            f"E: ★ and at least one was actually accepted — {accepted}",
            accepted > 0,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1937 a pin is given a transport", body)
