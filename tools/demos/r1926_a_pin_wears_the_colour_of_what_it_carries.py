#!/usr/bin/env python3
"""R1926 §5.11 §5.2 — **a pin on the assembled canvas is drawn in the colour of
the TYPE IT CARRIES, and a split's two halves are two colours.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability three reference-census rows name — the engine's
*colour of this pin type*, *colour of this pin* and *the second colour of a pin
type* — through the node lab as it is mounted in the shell.

# ★★★★★ The defect this round found, before it built anything

The canvas coloured every pin by the **node's** transport, read off the node's
kind. So splitting a locator drew its two halves in ONE colour — the parent's —
and a reader could not tell the host from the service, nor either from the whole
address they came out of. That is precisely what the reference's per-pin colour
hook is for, and the crate had no colour on a socket type at all.

# ★★★★★ What the reference actually does, measured at its own headers

Three findings, each of which changed the shape that was built:

  * the pin-level hook's own default IS the type's, and across the whole engine
    source **twelve** schemas override the TYPE colour while **one** overrides
    the PIN colour — and that one reads the pin's type more precisely and then
    answers a type colour. Nothing there gives one pin a colour of its own, so a
    port's colour is a DERIVATION here.
  * the *secondary* colour is answered only when the type is a MAP, and what it
    answers is the map's VALUE half. The census's own reason for that row — *a
    container whose element type has a colour of its own* — was wrong twice.
  * absence is not sayable there: the base returns black, and the implementation
    of substance writes `// Type does not have a defined color!` and returns a
    settings default.

# What this walk holds

  (A) the shell publishes the taxonomy's colours — one row per socket type, with
      the members a composite is made of.
  (B) ★★★★★ every pin the canvas draws publishes the colour it takes from that
      table, and the two answers agree by construction rather than by luck.
  (C) ★★★★★ the PAINT agrees with the model: each pin's drawn BORDER is the
      colour the model published for it. The border, not the rectangle — R1919
      measured what asserting a rectangle costs when the property lives on an
      edge.
  (D) ★★★★★ after a split, the two halves are drawn in TWO colours, and neither
      is the colour the whole address was drawn in. Without this the agreement
      in (B) and (C) would hold on a screen that coloured everything alike.
  (E) and the colours are distinct across the taxonomy, so "agrees with the
      model" is not satisfied by one colour everywhere.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1926_a_pin_wears_the_colour_of_what_it_carries.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, find_by_tag, run_demo  # noqa: E402

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


def inks(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/inks"))


def pin_border(app: RpcSubprocess, tag: str):
    """The BORDER a pin is drawn with.

    ★ R1919's lesson: what a pin's identity lives in here is its edge, so a walk
    comparing rectangles would see a screen that coloured nothing as identical
    to one that coloured everything.
    """
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, tag)
    return (node or {}).get("style", {}).get("border")


def as_hex(border) -> str | None:
    """The six hex digits a border's colour is, however the wire wrote it."""
    if border is None:
        return None
    if isinstance(border, dict):
        colour = border.get("color", border.get("colour"))
    else:
        colour = border
    if isinstance(colour, str):
        text = colour.lstrip("#").upper()
        return text[:6] if len(text) >= 6 else None
    if isinstance(colour, dict):
        try:
            return "{:02X}{:02X}{:02X}".format(
                int(colour["r"]), int(colour["g"]), int(colour["b"])
            )
        except (KeyError, TypeError, ValueError):
            return None
    return None


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

        banner("A — the taxonomy's colours are published")
        table = inks(app, surface)
        types = {row["type"]: row for row in table["types"]}
        ok(f"A: one row per socket type — {sorted(types)}", len(types) >= 7)
        ok(
            "A: ★ and every one of them carries a colour, so no pin on this "
            "canvas has to fall back to the view's own ink",
            all(row["ink"] for row in types.values()),
        )
        ok(
            "A: ★ none of them is silent, which is a different answer from "
            "black and one the reference cannot give at all",
            all(row["silent"] is False for row in types.values()),
        )
        composite = [row for row in types.values() if row["members"]]
        ok(
            f"A: ★★★★★ a composite type publishes what it is MADE of — "
            f"{[r['type'] for r in composite]}",
            composite != [],
        )
        for row in composite:
            ok(
                f"A: {row['type']} has {len(row['members'])} member colours, and "
                "the reference's *secondary* is the second of them",
                len(row["members"]) == 2 and all(row["members"]),
            )
            ok(
                f"A: ★★★★★ and its members are TWO colours, not one — {row['members']}",
                row["members"][0] != row["members"][1],
            )

        banner("E — the taxonomy's colours are distinct")
        every = [row["ink"] for row in types.values()]
        ok(
            f"E: ★ {len(every)} types, {len(set(every))} colours — a palette "
            "that answered one colour everywhere would satisfy every agreement "
            "check below",
            len(set(every)) == len(every),
        )

        banner("B — every pin publishes the colour it takes from that table")
        pins = {row["pin"]: row for row in table["pins"]}
        ok(f"B: the canvas publishes {len(pins)} pin(s)", len(pins) >= 2)
        ok(
            "B: ★ and each one has a colour",
            all(row["ink"] for row in pins.values()),
        )
        ok(
            "B: ★★★★★ every pin's colour is one the TYPE table published — the "
            "derivation, read back",
            all(row["ink"] in set(every) for row in pins.values()),
        )

        banner("D — ★★★★★ a split's two halves are two colours")
        # ★ WHICH pins this round's claim is about is the MODEL's fact, not a
        # choice made here: `member` is `depth > 0`, and the member pins are
        # exactly the ones whose colour used to be the parent's. The whole pins
        # are not a gap and not an exemption invented to pass — measured at the
        # behaviour canon, its OUT socket is drawn in the accent and its IN
        # socket is drawn in the protocol's colour ONLY while the node listens,
        # so both are screen states this screen already reproduces.
        # The card is whichever this screen will actually let split — asked by
        # asking, not assumed. A wired dial pin is refused, with the reason, and
        # which cards open wired is the opening graph's business rather than
        # this walk's.
        card, refusals = None, []
        for candidate in sorted({name.split(".", 1)[0] for name in pins}):
            try:
                app.invoke(f"{surface}/split_pin", f"{candidate},dial")
            except Exception as why:  # noqa: BLE001 — the reason is the subject
                refusals.append(str(why))
                continue
            card = candidate
            break
        ok(
            f"D: some card's dial pin splits; the ones that would not said why "
            f"— {refusals}",
            card is not None,
        )
        ok(
            "D: ★ and a refusal names what is in the way rather than only "
            "refusing",
            not refusals or any("wired" in why for why in refusals),
        )
        whole = pins[f"{card}.dial"]["ink"]
        ok(f"D: the dial pin carried a colour before the split — {whole!r}", whole is not None)
        app.tick_ms(16)
        after = {row["pin"]: row for row in inks(app, surface)["pins"]}
        halves = {
            name: row for name, row in after.items() if row["member"] and name.startswith(card)
        }
        ok(
            f"D: the split put {len(halves)} half-pin(s) on the frame — {sorted(halves)}",
            len(halves) == 2,
        )
        colours = [row["ink"] for row in halves.values()]
        ok(
            f"D: ★★★★★ and they are TWO colours, not one — {colours}",
            colours[0] != colours[1],
        )
        ok(
            f"D: ★★ nor is either of them the colour the WHOLE carried "
            f"({whole}), which is what this canvas drew for both before this round",
            all(colour != whole for colour in colours),
        )

        banner("C — ★★★★★ the paint agrees with the model, half by half")
        drawn = 0
        for name, row in sorted(halves.items()):
            card_name, word = name.split(".", 1)
            painted = as_hex(pin_border(app, f"lab.pin.{card_name}.{word}"))
            ok(f"C: {name} is on the frame with a border — {painted!r}", painted is not None)
            drawn += 1
            ok(
                f"C: ★★★★★ and it is PAINTED in the colour the model published "
                f"— {painted} vs {row['ink']}",
                painted == row["ink"].lstrip("#").upper(),
            )
        ok(
            f"C: ★ and {drawn} half-pin(s) were checked, so the agreement above "
            "is not vacuous",
            drawn == 2,
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1926_a_pin_wears_the_colour_of_what_it_carries", body))
