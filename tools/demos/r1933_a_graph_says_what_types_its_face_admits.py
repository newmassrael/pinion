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

#: A card whose dial pin carries no wire, so it can be split, AND whose
#: transport is named, so the face will take it. Named rather than discovered: a
#: walk that hunted for a splittable card would quietly assert about whichever
#: one it found.
#:
#: R1961 — it was `T-02`, which stopped satisfying the second half. A card reads
#: the transport it speaks off an address it uses — its own listen endpoint, or
#: the one it dials — and T-02 had neither, so it carried `Endpoint::Unspoken`
#: and the face refused it. R1961 moved the constant to `P-03`, which listened
#: and whose dial pin was then equally unwired.
#:
#: ⚠ Both of those sentences are HISTORY as of R2079 and are kept as history:
#: T-02 dials something now and P-03's dial pin is wired. The live reasoning is
#: the paragraph below.
#: ⚠⚠ ★★★★★ R2079 — `R-01`, where this said `P-03`, and the comment above it
#: recorded the premise that expired: *"`P-03` listens, and its dial pin is
#: equally unwired."* R2074 corrected `spec::LINKS` to name the dialler the
#: behaviour canon names, and `P-03` gained an outbound wire — so splitting its
#: dial pin is refused, correctly: *something is wired to Output port 0;
#: splitting would take away the place that wire lands*.
#:
#: Measured across every card on the canvas, exactly ONE can have its dial pin
#: split — the router, which dials nothing now and so carries no wire there.
#: The two properties this walk needs of the card are that the face TAKES its
#: whole dial address (section C) and that the pin is unwired so a HALF can be
#: asked for (section D), and the router is the only card with both.
FREE_CARD = "R-01"

#: R1961 — the card whose DIAL pin names no transport, because it dials nothing.
#:
#: ⚠⚠ ★★★★★ R2079 — `R-01`, where this said `T-02`, and the two swapped roles.
#: R2074 corrected `spec::LINKS` to name the dialler the behaviour canon names,
#: which left the router a pure sink and gave `T-02` an outbound wire. Measured
#: after that round:
#:
#:     dials nothing:   ['R-01']
#:     listens nowhere: ['T-01', 'Q-01', 'S-01', 'T-02', 'P-03']
#:
#: ⇒ ★ and the OLD comment's premise is gone with it: no card on this graph both
#: listens nowhere and dials nothing any more, so the sentence had to be split.
#: What this walk needs is the narrower fact — a dial pin with nothing to name a
#: transport from — which is exactly what a card that dials nothing has.
UNSPOKEN_CARD = "R-01"

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


def fresh_card(app: RpcSubprocess, surface: str) -> str:
    """Place a card from the palette and answer its name.

    ★ R2079 — a card just placed has been told nothing: no listen address, no
    dial address, so neither of its pins names a transport. That is the state
    this walk's `Unspoken` arm needs and the opening graph no longer has.

    The row is pressed by the ADDRESS the screen publishes for it (R2049), and
    a non-router role is chosen because a router may not be placed in every
    kind of graph (R1999) — the press would be refused for a reason that has
    nothing to do with what section C asks.

    ★★★★★ R2084 — and a role that does NOT open listening, read off the row's
    own `opens_listening`. The behaviour canon seeds a fresh card's fields per
    role and gives any seeded listen endpoint a free port, which this screen now
    reproduces — so *told nothing* stopped being true of every role. It is still
    true of most, and the screen publishes which, so this picks rather than
    guesses. Before that key existed this took the first non-Router role, which
    is Peer — one of the two the canon DOES seed.
    """
    spec = js(app.query(f"{surface}/spec"))
    row = next(
        r
        for r in spec["roles"]
        if r["name"] != "Router" and not r["opens_listening"]
    )
    before = {n.strip() for n in str(app.query(f"{surface}/nodes")).split(",") if n.strip()}
    app.click(path=row["tag"])
    app.tick_ms(16)
    after = {n.strip() for n in str(app.query(f"{surface}/nodes")).split(",") if n.strip()}
    made = sorted(after - before)
    assert len(made) == 1, f"pressing {row['tag']} placed {made}"
    return made[0]


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
            # R1961 — three refused, not two. The face admits an address whose
            # TRANSPORT IS NAMED, so it turns away the two halves of one AND
            # the whole address nothing has named a transport for (`locator`,
            # `Endpoint::Unspoken`) — a card just taken from the palette speaks
            # that until a wire tells it otherwise, and publishing it on the
            # graph's interface would offer a peer an address it cannot dial.
            f"A: ★ the refused ones are the halves of an address, and the "
            f"address with no transport named — {no}",
            set(no) == {"host", "service", "locator"},
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
        # R1961 — the other side of the same declaration, on a card that is not
        # a half of anything: a WHOLE address whose transport nothing has named
        # is refused too, because publishing it would offer a peer an address it
        # cannot dial. Before R1961 no such card existed — an escape hatch gave
        # every unnamed card TCP — so this arm of the face's rule was unreachable.
        #
        # ⚠⚠ ★★★★★ R2079 — **and the card is MADE, because R2074 left the graph
        # with none.** That round corrected `spec::LINKS` to name the dialler
        # the behaviour canon names, and the arm went unreachable a second time.
        # Measured before repairing, and both of the obvious re-pointings were
        # wrong:
        #
        #   * `T-02`, which this named, GAINED an outbound wire;
        #   * `R-01`, which now dials nothing, still has a taken dial pin —
        #     a card DECLARES a transport independently of any drawn wire, so
        #     *dials nothing* and *names no transport* are different facts.
        #
        # Asked of every card on the canvas, all eight answer `TAKEN`. What has
        # an unspoken dial pin is a card nobody has wired or configured yet — so
        # the walk presses a palette row and uses that, which is R2074's own
        # `fresh_card()` reasoning: when an assertion loses the state it stood
        # on, make the state rather than delete the assertion.
        made = fresh_card(app, surface)
        why = expose(app, surface, made, "dial")
        ok(
            f"C: ★★★★★ but {made}.dial is REFUSED — a card just placed has been "
            f"told no transport, so nothing says what that pin would speak — "
            f"{why!r}",
            why is not None and "does not admit" in (why or ""),
        )

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
