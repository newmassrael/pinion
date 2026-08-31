#!/usr/bin/env python3
"""R1933 §5.12 §5.2 — **a graph says which socket types its own face admits, and
the list it offers is the same list it judges by.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the two reference-census rows name — a tree type
asked whether a socket type is valid for it, and a schema asked whether a pin
type is supported — through the node lab as it is mounted in the shell.

# ★★★★★ The two rows were NOT one mechanism, and had to be measured apart

The slice spans both reference trees, and the standing warning applied:

  * **the DCC's is the real per-tree restriction** — a tree TYPE carries the
    hook, four tree types implement it (a shader tree answers a nine-member
    whitelist), and it is consumed three times: making an interface socket,
    retyping one, and an operator FINDING a type it may offer;
  * **the engine's is a chooser filter** — asked with a schema ACTION, supplied
    `true`, with ZERO overriders anywhere in its source, and one consumer: the
    pin-type selector widget filtering the list a person picks from.

⇒ two readings of ONE fact. The DCC reads it to REFUSE and to OFFER; the engine
only to offer. So one declaration with two readers is the shape here, and
writing the rule twice — once for the refusal, once for the list — is the
two-oracle defect R1924 and R1930 each paid for.

# What the lab declares, and why

This tool's face admits WHOLE addresses and not the halves a split makes. A
locator can be dialled; a bare host or a bare service cannot, so putting one on
the face would publish something no peer could connect to. That is the same
judgement the reference's shader tree makes when it lists what a shader graph
may carry.

# What this walk holds

  (A) the graph publishes what it admits, and the answer SPLITS — the whole
      addresses are in, the halves are out. A declaration that admitted
      everything, or nothing, would satisfy a one-sided check.
  (B) ★★★★★ the OFFER is exactly the admitted set: every type offered is one the
      edit takes, and every type refused is absent from the offer.
  (C) ★★★★★ a whole address goes on the face.
  (D) ★★★★★ a HALF is refused, by the crate's own declaration, and the refusal
      says which type — asserted for BOTH halves, so a rule that happened to
      catch one is not mistaken for the rule.
  (E) and nothing was left behind: the register's `unadmitted` list is empty
      before and after, so the refusals changed nothing.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1933_a_graph_says_what_types_its_face_admits.py
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

#: A card whose dial pin carries no wire, so it can be split. Named rather than
#: discovered: a walk that hunted for a splittable card would quietly assert
#: about whichever one it found.
FREE_CARD = "T-02"

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


def admits(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/admits"))


def expose(app: RpcSubprocess, surface: str, card: str, address: str):
    """Answer the refusal's sentence, or None when the face took it."""
    try:
        app.invoke(f"{surface}/expose_pin", f"{card},{address}")
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

        banner("A — the graph says what its face admits, and the answer SPLITS")
        reg = admits(app, surface)
        rows = {row["type"]: row["admitted"] for row in reg["types"]}
        ok(f"A: every socket type is judged — {sorted(rows)}", len(rows) >= 3)
        yes = sorted(ty for ty, taken in rows.items() if taken)
        no = sorted(ty for ty, taken in rows.items() if not taken)
        ok(
            f"A: ★★★★★ BOTH sides are populated — admitted {yes}, refused {no}. A "
            "declaration that took everything, or nothing, would satisfy a "
            "one-sided check",
            yes != [] and no != [],
        )
        ok(
            f"A: ★ the refused ones are the HALVES of an address — {no}",
            set(no) == {"host", "service"},
        )
        ok(
            f"A: ★ and the admitted ones are whole locators — {yes}",
            all(ty.startswith("locator/") for ty in yes),
        )

        banner("B — ★★★★★ the OFFER is exactly the admitted set")
        offers = reg["offers"]
        ok(f"B: the graph offers a list — {offers}", offers is not None)
        ok(
            f"B: ★★★★★ every offered type is one the face admits — {offers}",
            all(rows.get(ty) is True for ty in offers),
        )
        ok(
            f"B: ★★★★★ and every refused type is absent from the offer — {no}",
            all(ty not in offers for ty in no),
        )
        ok(
            "B: ★ the two sets are equal, not merely nested — an offer that "
            "dropped an admitted type would pass the line above",
            sorted(offers) == yes,
        )

        banner("C — ★★★★★ a WHOLE address goes on the face")
        ok(
            f"C: nothing is unadmitted to begin with — {reg['unadmitted']}",
            reg["unadmitted"] == [],
        )
        said = expose(app, surface, FREE_CARD, "dial")
        ok(f"C: ★★★★★ {FREE_CARD}.dial is taken — {said!r}", said is None)

        banner("D — ★★★★★ a HALF is refused, and the refusal says which type")
        apart = app.invoke(f"{surface}/split_pin", f"{FREE_CARD},dial")
        app.tick_ms(16)
        ok(f"D: the pin comes apart first — {apart!r}", isinstance(apart, str))
        for half, expected in (("dial.host", "Host"), ("dial.service", "Service")):
            why = expose(app, surface, FREE_CARD, half)
            ok(f"D: ★★★★★ {half} is REFUSED — {why!r}", why is not None)
            ok(
                f"D: ★★★★★ and the refusal names the type — {expected!r} in {why!r}",
                expected in (why or ""),
            )
            ok(
                f"D: ★ it is the FACE that refused, not the pin lookup — {why!r}",
                "does not admit" in (why or ""),
            )

        banner("E — the refusals changed nothing")
        after = admits(app, surface)
        ok(
            f"E: ★ the face still carries nothing it does not admit — "
            f"{after['unadmitted']}",
            after["unadmitted"] == [],
        )
        ok(
            "E: ★ and the declaration is what it was — a refusal does not "
            "narrow or widen the set",
            after["offers"] == offers,
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1933_a_graph_says_what_types_its_face_admits", body))
