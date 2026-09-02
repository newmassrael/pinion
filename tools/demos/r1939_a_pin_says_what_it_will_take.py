#!/usr/bin/env python3
"""R1939 §5.2 §5.11 — **a pin says what it will take, and hands back the
address it would have taken.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — what may REST at a
port — as the node lab is mounted in the shell.

# ★★★★★ The measurement that reversed this row's verdict

The row's own recorded reason said the absence was DELIBERATE, and it was read
off the reference's signature: a node is asked for a pin name AND A KEY and
answers a string, which is an open bag of untyped metadata, and a bag of
untyped strings is where a model goes to stop being checkable.

Read from its CONSUMERS it is not a bag. Twenty-one call sites ask eighteen
distinct keys and every one asks the same question — *what may rest at this
port, and how should an editor offer it?* Four want a numeric range, nine a
filter on what may be picked, one a closed list of options, four how to present
the field. All eleven overriders reach the SAME lookup: from the pin to the
declaration it was generated from, falling back to its parent. ⚠ Not all by
CHAINING — nine call up, the tenth IS that lookup, and the eleventh chains to
nothing and runs the same lookup against its own model. Four add a case of
their own beside it — three a fixed string for one pin-and-key pair, one built
from the graph — and only TWO of those sit AHEAD of the lookup, the other two
being a fallback taken only when it answered empty. Not one of the eleven reads
a store hung on the port, so nobody authors that metadata on a port, which is
why the declaration lives on the kind here.

★ Those two qualifications are R1939 correcting its OWN sentence before
publishing it: re-measured clause by clause in the closing audit, "all eleven
CHAIN" and "four add a case ahead" were each half false — and each was wrong in
the direction that STRENGTHENS the conclusion, which is why nothing would have
prompted a reader to check. Absence there is spelled as the empty string, so *no such
key* and *the key says nothing* are one value. And one shipped overrider
IGNORES the key it is asked for, answering one fixed key's value for every
question put to it — a defect nothing there can catch, because a string key is
checked against nothing.

⇒ the bag is not built. The capability is, typed, and the refusal carries the
value the same declaration would have taken.

# What this walk holds

  (A) the journey reaches the node lab, and every pin says what it will take,
      in a sentence, without being handed a value first.
  (B) ★ the sentence is the PIN's and not one global rule — a pin names the
      transport it speaks, so two cards of different transports want different
      addresses at the same pin address, and ONE card wants different addresses
      at its two pins.

# ★★★★★ R1975 — what (B) drives, and why it changed

(B) used to make the difference by choosing a transport on the subject's own
DIAL pin. Measured at R1975 through this very surface, that call changed
nothing on the canvas while answering success, for two reasons that had been
hidden behind a third:

  * a dial's transport is not the card's fact — it is read off the endpoint the
    wire lands on, which is the behaviour canon's model exactly (its node has no
    dial scheme; its verb for re-choosing one is on the LINK). So the write had
    a second author and the derivation won.
  * every wire already landed carried a *copy* of the peer's address, so
    re-scheming the card left the copies behind and no pin moved.
  * and it had appeared to work only while the type relation severed the wire
    that would have re-derived it — a rule R1969 removed after measuring that
    the canon has no such constraint. Removing it is what brought this walk
    down, which is the honest way round: the walk was leaning on a defect.

So (B) now drives the edit the canon HAS — a card chooses what it ACCEPTS — and
asserts the chain that follows from it, which is a stronger claim than the one
it replaced: an edit on one card moves a pin sentence on another.
  (C) ★★★★★ the edit REFUSES what the pin will not take, and nothing changed.
  (D) ★★★★★ the refusal hands back the address the same declaration WOULD
      take, and taking it up is accepted — the permission and the repair are
      one answer.
  (E) ★ a pin drawn as one transport refuses another transport's address and
      re-schemes it, which is what makes the type part of the rule rather than
      beside it.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1939_a_pin_says_what_it_will_take.py
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


def takes(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/takes"))["pins"]


def pin_of(rows: list[dict], card: str, which: str) -> dict:
    return next(row for row in rows if row["card"] == card and row["pin"] == which)


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

        banner("A — every pin says what it will take, in a sentence")
        rows = takes(app, surface)
        ok(f"A: the register answers — {len(rows)} pin(s)", len(rows) > 1)
        ok(
            "A: ★ every row carries a sentence, so a screen has something to "
            "put beside the field before anybody types",
            all(row["wants"] for row in rows),
        )
        ok(
            "A: ★ and every row says whether what it rests at STANDS, which is "
            "the fact a form needs to decide whether to mark the field",
            all("stands" in row for row in rows),
        )
        # ★★★★★ The opening canvas is CLEAN by its own declaration, which is
        # what makes every refusal below a caused one rather than a pre-existing
        # state this walk happened to find.
        unstanding = [row for row in rows if not row["stands"]]
        ok(
            f"A: ★★★★★ every address this canvas already rests at is one its "
            f"own pin admits — {len(unstanding)} that do not: {unstanding}",
            not unstanding,
        )

        banner("B — ★ the sentence is the PIN's, not one global rule")
        dials = [row for row in rows if row["pin"] == "dial"]
        # ★★★★★ R1975 — TWO cards are named now, because the edit and the pin it
        # moves are on different cards, and that is the behaviour canon's own
        # shape rather than an indirection this walk chose.
        #
        # A card chooses what it ACCEPTS — the addresses it listens on. What a
        # card DIALS is not its own fact at all: the wire lands on one of the
        # peer's endpoints and speaks that endpoint's scheme, which is why the
        # canon's node has no dial scheme and its verb for re-choosing one is on
        # the LINK. So one edit on the listener moves the sentence of every card
        # dialling it, and this walk asserts exactly that chain.
        #
        # ⚠ Until R1975 this section drove `set_pin_transport(R-01, dial, udp)`
        # and asserted R-01's own dial sentence changed. Measured at R1975, that
        # call changed NOTHING a person could see while answering success — the
        # dial write was overwritten by the derivation, and the listen half's
        # re-scheming never reached the pins because every wire already landed
        # carried a stale copy of the address. It had appeared to work only
        # while `conversion` severed the wire that would have re-derived it,
        # which R1969 removed after measuring that the canon has no such rule.
        listener = "R-01"
        subject = "P-01"
        # R1961 — ⚠ this used to read *the opening canvas speaks ONE transport,
        # so the sentences start identical*, and that sentence WAS the defect
        # `debt-every-card-on-the-opening-graph-speaks-one-transport` is open on,
        # written down as an assertion.
        #
        # R1962 — and it is THREE now, which is the debt actually moving: a tcp
        # sentence, a quic one (P-01 listens on quic and the cards that dial it
        # read that), and the typeless one belonging to the card that speaks
        # nothing at all. Asserted as the three KINDS rather than as a count, so
        # a fixture that produced three of the same kind could not satisfy it.
        wants = {d["wants"] for d in dials}
        ok(
            f"B: ★★★★★ the opening canvas speaks more than one transport, and "
            f"one card speaks NOTHING — {sorted(wants)}",
            any("tcp/host:service" in w for w in wants)
            and any("quic/host:service" in w for w in wants)
            and any("any value of" in w for w in wants),
        )
        # ★ So the difference is CAUSED rather than found: R1937's verb makes
        # one card accept another transport, and every sentence that reads off
        # that address has to follow.
        was = pin_of(rows, subject, "dial")["wants"]
        said = app.invoke(f"{surface}/set_pin_transport", f"{listener},accept,udp")
        app.tick_ms(16)
        ok(
            f"B: ★★★★★ the answer says what happened to the WIRES, not just to "
            f"the card — {said!r}",
            "wire(s) followed the address" in str(said),
        )
        rows = takes(app, surface)
        dials = [row for row in rows if row["pin"] == "dial"]
        ok(
            f"B: ★★★★★ and now they disagree — "
            f"{sorted({d['wants'] for d in dials})}",
            len({d["wants"] for d in dials}) > 1,
        )
        # ★★★★★ The listener's own pins first: an accept pin's type is the
        # landing item's, so a wire that kept a stale copy of the address is a
        # pin drawn in a transport the card does not speak. That was the defect.
        accepting = [
            row for row in rows if row["card"] == listener and row["pin"] == "accept"
        ]
        ok(
            f"B: ★★★★★ every accept pin of the card that was edited says the "
            f"new transport — {sorted({row['wants'] for row in accepting})}",
            len(accepting) > 1
            and all("udp/host:service" in row["wants"] for row in accepting),
        )
        # ★★★★★ And the chain: the pin that moved is on ANOTHER card, because a
        # dial reads its scheme off the endpoint it lands on.
        now = pin_of(rows, subject, "dial")["wants"]
        ok(
            f"B: ★★★★★ a card that DIALS the edited one followed it, without "
            f"being edited — {was!r} -> {now!r}",
            "udp/host:service" in now and was != now,
        )
        # ★★★★★ Two pins of ONE card wanting different addresses is the sharpest
        # form of this section's claim: no global rule can produce it.
        subject_accept = pin_of(rows, subject, "accept")["wants"]
        ok(
            f"B: ★★★★★ and the SAME card wants different addresses at its two "
            f"pins — accept {subject_accept!r} vs dial {now!r}",
            subject_accept != now,
        )
        # ★ The other half of the model, stated as a refusal: what a card dials
        # is the wire's fact, so the verb declines rather than answering a
        # success the screen does not have.
        why = refusal(app, f"{surface}/set_pin_transport", f"{listener},dial,tcp")
        ok(
            f"B: ★★★★★ and a DIAL is refused, with the repair named — {why!r}",
            why is not None
            and "scheme of the endpoint it lands on" in str(why)
            and "choose the link's endpoint" in str(why),
        )

        banner("C — ★★★★★ the edit refuses what the pin will not take")
        before = pin_of(takes(app, surface), subject, "dial")["carries"]
        why = refusal(app, f"{surface}/set_pin_locator", f"{subject},dial,not-an-address")
        ok(f"C: ★★★★★ the edit was refused — {why!r}", why is not None)
        ok(
            f"C: ★ and the refusal quotes the PIN'S OWN sentence rather than a "
            f"generic one — {why!r}",
            "an address this pin can speak" in str(why),
        )
        app.tick_ms(16)
        ok(
            "C: ★★★★★ and NOTHING changed: a refused edit is not a partial one",
            pin_of(takes(app, surface), subject, "dial")["carries"] == before,
        )

        banner("D — ★★★★★ the refusal hands back the address it WOULD take")
        # ★ A whole locator of another transport: the host and the service are
        # right and the scheme is not, which is the commonest real mistake.
        why = refusal(
            app, f"{surface}/set_pin_locator", f"{subject},dial,zzz/10.0.0.9:7001"
        )
        ok(f"D: ★ refused again — {why!r}", why is not None)
        ok(
            f"D: ★★★★★ and the refusal NAMES the address it would take, host "
            f"and service kept — {why!r}",
            "10.0.0.9:7001" in str(why) and "such as" in str(why),
        )
        # The repair, read off the refusal rather than guessed, is what gets
        # applied — so this cannot pass if the two came from different places.
        offered = str(why).split("such as ")[-1].strip().strip('"').strip("'")
        offered = offered.rstrip('."\'')
        said = app.invoke(f"{surface}/set_pin_locator", f"{subject},dial,{offered}")
        app.tick_ms(16)
        ok(
            f"D: ★★★★★ the value the SAME declaration offered is accepted — "
            f"{said!r}",
            "now rests at" in str(said),
        )
        landed = pin_of(takes(app, surface), subject, "dial")
        ok(
            f"D: ★ and the register agrees it stands now — {landed}",
            landed["carries"] == offered and landed["stands"] is True,
        )

        banner("E — ★ the transport is part of the rule")
        ok(
            f"E: ★ the repair re-schemed rather than keeping what was typed — "
            f"{offered!r}",
            offered.endswith("/10.0.0.9:7001") and not offered.startswith("zzz/"),
        )
        ok(
            f"E: ★★★★★ and the scheme it chose is the one THIS pin now speaks, "
            f"not the one the canvas opened with — {offered!r}",
            offered.startswith("udp/"),
        )
        ok(
            f"E: ★ which is the sentence the same pin publishes — "
            f"{landed['wants']!r}",
            offered.split("/", 1)[0] in landed["wants"],
        )
        # R1961 — and the other side of the same rule: a card whose transport is
        # read off the peer it dials has no address of its own to re-scheme, so
        # the verb refuses it by name instead of appearing to work.
        speechless = [d["card"] for d in dials if "can speak" not in d["wants"]]
        ok(
            f"E: ★★★★★ the card that speaks nothing is named by the register — "
            f"{speechless}",
            len(speechless) == 1,
        )
        # ⚠ R1975 — `dial` until R1975, which made that address a refusal of its
        # own on every card. The claim here is about a card with NO address, so
        # it is asked on the side the verb owns; the dial refusal is B's.
        why = refusal(
            app, f"{surface}/set_pin_transport", f"{speechless[0]},accept,udp"
        )
        ok(
            f"E: ★★★★★ and choosing a transport for it is REFUSED, because the "
            f"choice moves an address it does not have — {why!r}",
            why is not None and "listens nowhere" in str(why),
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1939 a pin says what it will take", body)
