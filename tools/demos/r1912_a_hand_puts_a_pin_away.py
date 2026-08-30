#!/usr/bin/env python3
"""R1912 §5.32 §5.12 §2 #2 §2 #7 — **a hand can put a node's pin away by name,
and the tool says WHY a pin is not on the frame.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives the campaign
`debt-node-system-coverage-campaign`, whose census names four operators across
two node references that nothing here could express.

# ★★★★★ What the entry re-measurement found, and it re-cut the census's chunk

The campaign file carried "the largest remaining chunk is **struct pin 8**"
(five graph-editor commands plus the schema's split/recombine and the node's
can-split). Re-running `tools/reference_census.py` this round returns exactly
those eight as ABSENT, so the count was right — and reading the engine's own
source, the CHUNK IS TWO MECHANISMS:

* five of them are split/recombine, whose model is a pin with SUB-PINS and a
  parent, one pin per member of a composite value;
* three of them — *remove this struct var pin*, *remove all other pins*,
  *restore all structure pins* — touch none of that. Measured in the engine's
  editor source, they call the node's own remove-field-pins with a
  given-pin / all-other-pins selector and a restore-all, gated on "not all pins
  are shown". That is **hiding**, not splitting.

⇒ and the three belong with a FOURTH row on the other reference. The DCC's
socket-hide operator sets a per-socket user-hidden flag over the unwired
sockets, and its own model asks

    is_visible() = !is_user_hidden() && is_available() && inferred_visibility()

— **three independent reasons a socket is not drawn**, of which exactly one is
a person's. So four census rows across two references were ONE absent
mechanism, which is R1632's lesson arriving from the opposite direction: that
round found one chunk was the same mechanism as items elsewhere; this one found
one chunk was two mechanisms.

# What this crate had, and why it was not the same thing

`Appearance::hide_unused_ports` is a rule over the WIRING, evaluated on every
read. It can only ever hide what nothing is wired to, and it re-decides — so a
person cannot be *told to* keep a particular port away, and a port they hid
comes back the moment something is wired to it. R1912 adds the declaration:
`Document::put_away_ports` with the references' three scopes as one parameter,
`restore_ports`, and `VisiblePorts::why_hidden`, which is the question neither
reference can be asked.

# Superior to both, and it is a measurement rather than a preference

The DCC's operator OVERWRITES what a person chose the last time they pressed
it, because it derives the set from the wiring every time; here the request is
remembered by name and survives that port being wired. The engine's three live
on one node class; here it is the model's verb, so any kind has it. And the
state where a node has no pin on the frame at all — which the DCC's own
operator reaches on an unwired node, so it is NOT refused here either — is
PUBLISHED (`nothing_drawn`) rather than left for a reader to notice.

# What this walk holds

  (A) the assembled tool mounts the lab, and the lab publishes each card's
      pins with a reason word for every one that is not drawn.
  (B) the inspector's seat — the DCC's own bulk toggle — puts the unwired pins
      away, and they leave the FRAME, not merely a field.
  (C) the seat is a TOGGLE: pressed again it brings them back, which is the
      reference's own shape and the half a scope word cannot carry.
  (D) the engine's two per-pin scopes name one pin, and the frame agrees.
  (E) a put-away pin STAYS away when the node's derived rule would draw it —
      the claim the rule alone can never make.
  (F) a refusal names what was asked, so a client can correct it.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1912_a_hand_puts_a_pin_away.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_action_refused,
    run_demo,
    walk_nodes,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1440, 900)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    """A published value, whether the surface handed back JSON or a string."""
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    """Where the screen mounted at `seat` answers, as the application says.

    Asked rather than composed (R1890): the roster publishes each mounted
    destination's address, so this never has to know the shape of it.
    """
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def cards(app: RpcSubprocess, surface: str) -> dict:
    """Every card the lab publishes, by name."""
    return js(app.query(f"{surface}/cards"))


def painted(app: RpcSubprocess) -> set[str]:
    """Every tag on the ASSEMBLED tool's frame."""
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    return {found["tag"] for _, found in walk_nodes(snap) if found.get("tag")}


def pin_tags(name: str) -> tuple[str, str]:
    return f"lab.pin.{name}.dial", f"lab.pin.{name}.accept"


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        # The lab is mounted when a reader goes there, so the journey is part of
        # the walk: a claim about a screen nobody can reach is a claim about a
        # binary nobody runs.
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("A — every card publishes its pins, with a reason for each")
        rows = cards(app, surface)
        ok(f"A: the lab publishes {len(rows)} card(s)", len(rows) > 1)
        for name, row in rows.items():
            pins = row.get("pins")
            ok(
                f"A: {name} publishes its pins — {pins}",
                isinstance(pins, dict)
                and {"dial", "accept", "wired", "nothing_drawn"} <= set(pins),
            )
        drawn_now = {n: r["pins"] for n, r in rows.items()}
        print(f"    {json.dumps(drawn_now, ensure_ascii=False)[:400]}")

        # ⚠ The subject is DERIVED from what the tool publishes, not assumed.
        # A first draft picked the first card with a drawn dial pin and asserted
        # the bulk scope would take something; on this graph every one of that
        # card's pins is wired, so the scope correctly selected nothing and the
        # walk was asserting about a population it had not measured.
        subject = next(
            (
                n
                for n, r in rows.items()
                if r["pins"]["dial"] == "drawn" and len(r["pins"]["wired"]) < 2
            ),
            None,
        )
        ok(
            f"A: a card with a drawn dial pin AND an unwired pin exists to act "
            f"on — {subject}",
            subject is not None,
        )
        app.invoke(f"{surface}/select", subject)
        app.tick_ms(16)
        dial_tag, accept_tag = pin_tags(subject)
        frame = painted(app)
        ok(
            f"A: {subject}'s dial pin is on the frame ({dial_tag})",
            dial_tag in frame,
        )

        banner("B/C — the inspector's seat is the reference's own toggle")
        before = painted(app)
        said = app.invoke(f"{surface}/put_away_pins", f"{subject},unwired")
        app.tick_ms(16)
        after = painted(app)
        gone = {t for t in (dial_tag, accept_tag) if t in before and t not in after}
        ok(
            f"B: the bulk scope took {len(gone)} pin(s) off the FRAME — {said}",
            bool(gone),
        )
        state = cards(app, surface)[subject]["pins"]
        ok(
            f"B: and the tool says WHY each one is gone — {state}",
            any(state[p] == "put_away" for p in ("dial", "accept")),
        )
        app.invoke(f"{surface}/put_away_pins", f"{subject},unwired")
        app.tick_ms(16)
        ok(
            "C: pressed again it brings them back — the reference's toggle, "
            "which is the half a scope word cannot carry",
            painted(app) >= gone,
        )

        banner("D — the engine's two per-pin scopes")
        app.invoke(f"{surface}/put_away_pins", f"{subject},dial")
        app.tick_ms(16)
        ok(
            "D: *remove this pin* named ONE pin and the frame agrees",
            dial_tag not in painted(app),
        )
        ok(
            "D: and it is that pin's row that says a hand did it",
            cards(app, surface)[subject]["pins"]["dial"] == "put_away",
        )
        app.invoke(f"{surface}/put_away_pins", f"{subject},restore")
        app.tick_ms(16)
        ok(
            "D: *restore all* brings it back",
            dial_tag in painted(app),
        )
        app.invoke(f"{surface}/put_away_pins", f"{subject},others:dial")
        app.tick_ms(16)
        kept = cards(app, surface)[subject]["pins"]
        ok(
            f"D: *remove all other pins* kept exactly the named one — {kept}",
            kept["dial"] == "drawn",
        )
        ok(
            "D: and the frame kept it too",
            dial_tag in painted(app),
        )

        banner("E — a put-away pin stays away where the derived rule would draw it")
        # Collapse asks the node to hide UNUSED ports, which is the rule. The
        # dial pin is drawn under it (it is not put away); the accept pin is
        # put away and would come back the moment it were wired. What this
        # checks is that the reason does not change under the rule.
        app.invoke(f"{surface}/collapse", subject)
        app.tick_ms(16)
        under_rule = cards(app, surface)[subject]["pins"]
        ok(
            f"E: under the node's own rule the reasons stay apart — {under_rule}",
            under_rule["accept"] in ("put_away",),
        )
        app.invoke(f"{surface}/collapse", subject)
        app.invoke(f"{surface}/put_away_pins", f"{subject},restore")
        app.tick_ms(16)

        banner("F — a refusal names what was asked")
        sentence = assert_action_refused(
            lambda: app.invoke(f"{surface}/put_away_pins", f"{subject},elbow"),
            saying="elbow",
        )
        ok(f"F: an unknown pin is refused by name — {sentence}", "elbow" in sentence)
        sentence = assert_action_refused(
            lambda: app.invoke(f"{surface}/put_away_pins", "dial"),
            saying="scope",
        )
        ok(f"F: and a request with no scope says so — {sentence}", bool(sentence))

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1912_a_hand_puts_a_pin_away", body))
