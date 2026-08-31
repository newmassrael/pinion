#!/usr/bin/env python3
"""R1928 §5.12 §5.2 — **a port says what THIS node calls it, and who chose the
name.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the two reference-census rows name — *should this
node override its pin names* and *what is this pin called* — through the node
lab as it is mounted in the shell, at the place a name is actually delivered to
a person: the accessibility tree.

# ★★★★★ What the reference does, measured at its header, its ONE consumer and
# ALL SIX of its overriders

Its graph node publishes a bool and a text, and the header says how they
compose: if the bool answers yes, the text hook is called *for each pin, each
frame*. One consumer, the schema's display-name call, takes the whole ordinary
naming path as its `else`. Reading every overrider gave three findings, and each
changed what was built:

  * **four of the six use it to take a name AWAY**, not to give a different one
    — two reroute classes answer the empty text for every pin with the comment
    *keep the pin size tiny*, a setter node answers it for its output and its
    control pins, and a fourth answers the bool alone;
  * **nobody names a pin per pin** — five of the six ignore the pin they are
    handed, and the sixth reads it only to decide whether to suppress;
  * 🟥 **"show nothing" and "I have nothing to say" are one value there**, and a
    class sits on the ambiguity: it overrides the bool and never the text, so
    the base class's empty default suppresses every one of its pin names.
    Whether that is intent or omission cannot be told from the source.

⇒ so the answer here is a THREE-arm type — keep / rename / silent — and not
R1927's `Option`. There the empty answer meant nothing; here it is the
commonest thing the capability is for.

# ★★★★★ And the census sentence was half false, which the entry measurement found

The pin read *a port's name comes from the kind and a node cannot override it*.
A node has been able to name a port since R1632: `Item::label` names one item of
a variadic run, and this very screen uses it for the address each accept slot
listens on. What was genuinely absent is a node naming a FIXED port, any way to
say a port shows NO name, and any way to tell the three sources apart.

# What this walk holds

  (A) the shell publishes one row per port of every card, with the resolved
      name and its SOURCE — and all three sources are present on the opening
      canvas, so none of them is a branch nothing reaches.
  (B) ★★★★★ a pin's spoken sentence CARRIES the model's name. The accessibility
      tree is where a name is delivered to a person who cannot see the canvas,
      and before this round it said the same six words on every accept pin of
      every card.
  (C) ★★★★★ a SILENT port is announced as unnamed — the pin is still there,
      still says what it is for, and what is absent is said in as many words.
  (D) ★★★★★ the name FOLLOWS the model: give a card an address to listen on and
      its accept pin stops being unnamed, in the register and in the sentence
      together.
  (E) and the two never disagree, for every card at once, because the sentence
      is derived from the register rather than written beside it.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1928_a_port_says_what_this_node_calls_it.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, access_node_by_tag, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"

#: The card this walk gives an address to. Named rather than discovered so the
#: walk fails loudly if the specification's opening graph changes shape under
#: it, instead of quietly finding another card and asserting something else.
SILENT_CARD = "S-01"
#: An address in the transport the opening graph already uses, so the edit
#: changes what this walk is about and nothing else.
NEW_ADDRESS = "tcp/0.0.0.0:7460"

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


def register(app: RpcSubprocess, surface: str) -> list[dict]:
    """One row per port of every card: the resolved name and its source."""
    return js(app.query(f"{surface}/port_names"))["ports"]


def first_port(rows: list[dict], card: str, side: str) -> dict | None:
    for row in rows:
        if row["card"] == card and row["side"] == side and row["index"] == 0:
            return row
    return None


def access(app: RpcSubprocess) -> dict:
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result


def spoken(app: RpcSubprocess, card: str, side: str) -> str | None:
    """What a reader who cannot see the canvas is told about that pin."""
    node = access_node_by_tag(access(app), f"lab.pin.{card}.{side}")
    return None if node is None else node.get("name")


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

        banner("A — every port's name, and who chose it")
        rows = register(app, surface)
        ok(f"A: the register has rows — {len(rows)}", len(rows) >= 6)
        sources = {row["source"] for row in rows}
        ok(
            f"A: ★★★★★ all THREE sources are reached on the opening canvas — "
            f"{sorted(sources)}. A branch nothing reaches is a branch that can "
            "say anything",
            sources == {"kind", "item", "node"},
        )
        by_kind = [r for r in rows if r["source"] == "kind"]
        by_item = [r for r in rows if r["source"] == "item"]
        by_node = [r for r in rows if r["source"] == "node"]
        ok(
            f"A: ★ the KIND's own declaration names the dial pins — "
            f"{sorted({r['name'] for r in by_kind})}",
            all(r["side"] == "dial" for r in by_kind) and by_kind != [],
        )
        ok(
            "A: ★★★★★ an ITEM's authored label names an accept slot for the "
            f"ADDRESS it listens on — {sorted({r['name'] for r in by_item})}",
            by_item != [] and all("/" in (r["name"] or "") for r in by_item),
        )
        ok(
            f"A: ★★★★★ and the NODE's own answer is SILENT — "
            f"{sorted({r['card'] for r in by_node})}",
            by_node != [] and all(r["name"] is None for r in by_node),
        )
        ok(
            "A: ★ a name is never the empty string: absence is `null`, which is "
            "the distinction the reference's empty text cannot make",
            all(r["name"] != "" for r in rows),
        )

        banner("B — ★★★★★ the spoken sentence CARRIES the model's name")
        named = [r for r in rows if r["name"] and r["index"] == 0]
        ok(f"B: there are named first ports to check — {len(named)}", named != [])
        for row in named:
            said = spoken(app, row["card"], row["side"])
            ok(
                f"B: {row['card']}'s {row['side']} pin is announced, and the "
                f"sentence carries {row['name']!r} — {said!r}",
                isinstance(said, str) and row["name"] in said,
            )
        heard = {spoken(app, r["card"], "accept") for r in by_item if r["index"] == 0}
        ok(
            f"B: ★★★★★ and the accept pins no longer all say the same thing — "
            f"{len(heard)} distinct sentence(s) over {len([r for r in by_item if r['index'] == 0])} card(s)",
            len(heard) > 1,
        )

        banner("C — ★★★★★ a SILENT port is announced as unnamed")
        silent = first_port(rows, SILENT_CARD, "accept")
        ok(
            f"C: {SILENT_CARD}'s accept port is the node's own silent answer — "
            f"{silent!r}",
            silent is not None
            and silent["name"] is None
            and silent["source"] == "node",
        )
        hushed = spoken(app, SILENT_CARD, "accept")
        ok(
            f"C: ★ the pin is STILL announced — suppressing a name is not "
            f"removing a pin — {hushed!r}",
            isinstance(hushed, str) and hushed.strip() != "",
        )
        ok(
            "C: ★★★★★ and the sentence says it is unnamed rather than carrying "
            f"an empty one — {hushed!r}",
            "unnamed" in (hushed or ""),
        )
        ok(
            "C: ★ it still says what the pin is FOR, which is the half a "
            f"suppressed name must not take with it — {hushed!r}",
            "drop a link here" in (hushed or ""),
        )

        banner(f"D — ★★★★★ the name FOLLOWS the model: give {SILENT_CARD} an address")
        app.invoke(f"{surface}/select", SILENT_CARD)
        held = app.invoke(f"{surface}/set_field", f"listen.endpoints={NEW_ADDRESS}")
        app.tick_ms(16)
        ok(f"D: the field took it — {held!r}", isinstance(held, str))
        after = register(app, surface)
        now = first_port(after, SILENT_CARD, "accept")
        ok(
            f"D: ★★★★★ its accept port is no longer the node's silent answer — "
            f"was {silent!r}, now {now!r}",
            now is not None and now["source"] != "node" and now["name"] is not None,
        )
        said = spoken(app, SILENT_CARD, "accept")
        ok(
            f"D: ★★★★★ and the SENTENCE moved with it, so the paint is a "
            f"rendering of the model rather than a second answer — {said!r}",
            isinstance(said, str) and "unnamed" not in said,
        )
        ok(
            f"D: ★ the other silent card is untouched, so the change was this "
            "card's and not a repaint of everything",
            any(
                r["source"] == "node" and r["name"] is None
                for r in after
                if r["card"] != SILENT_CARD
            ),
        )

        banner("E — the register and the sentences are one answer")
        final = register(app, surface)
        for row in final:
            if row["index"] != 0:
                continue
            said = spoken(app, row["card"], row["side"])
            if said is None:
                continue
            if row["name"] is None:
                ok(
                    f"E: {row['card']} {row['side']} — the register says unnamed "
                    f"and so does the sentence: {said!r}",
                    "unnamed" in said,
                )
            else:
                ok(
                    f"E: {row['card']} {row['side']} — the register says "
                    f"{row['name']!r} and the sentence carries it: {said!r}",
                    row["name"] in said and "unnamed" not in said,
                )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1928_a_port_says_what_this_node_calls_it", body))
