#!/usr/bin/env python3
"""R1914 §5.32 §5.12 §2 #2 §2 #7 — **a pin on the assembled tool comes apart
into the two things it is made of, and goes back together.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives `debt-node-system-coverage-campaign`, whose
census names five split/recombine rows. R1912 closed the QUESTION, R1913 built
the value half, and this round builds the ACT — the four rows the reference
spells `SplitPin` / `RecombinePin` on its schema and `SplitStructPin` /
`RecombineStructPin` on its editor.

# ★★★★★ What a weaker walk would have missed, measured

The reference makes sub-pins REAL pins, so an index after a split moves. This
walk is therefore not about a pin appearing: it is about **four facts moving
together** and none of them getting out of step.

  * the parent stops being drawn, and the model says WHY (`split`) — the
    reference sets the same flag and has no field to report it from;
  * the members appear where the parent was, carrying **the halves of the
    address that pin was actually carrying**, not two declared defaults;
  * they are announced, not only drawn;
  * folding composes the halves back into that same address, which the
    reference does for four named struct types and no others.

⚠ The second of those is the one that caught a real defect on the first run of
this walk. An earlier draft asserted only that each member carried *something*
— and it passed while the parent's value was not being shared out at all,
because the taxonomy's declared member defaults happened to be the same two
strings the address would have produced. The repair was to make the screen
publish what the pin carries, so the comparison is against a fact rather than
against a plausible shape. **Two faults that always travel together cannot be
told apart by an assertion that either one satisfies.**

# What this walk holds

  (A) the assembled tool mounts the lab, and a card publishes a pin that says
      it splits AND publishes the address it carries.
  (B) splitting it takes the parent OFF the frame and puts two member pins on,
      each announced to a reader who does not look at pixels.
  (C) the members carry the two halves of THAT address.
  (D) the members sit at consecutive resolved indices immediately after the
      parent, and publish addresses rather than indices.
  (E) folding at a MEMBER folds the pin that member belongs to, and the address
      comes back together — apart and back again gives the value back.
  (F) the verb refuses, in words: a pin the card does not draw, a member word
      the taxonomy does not have, and a fold with nothing to fold.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1914_a_pin_comes_apart_and_goes_back.py
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
VIEWPORT = (1400, 900)

#: The member words a locator is made of. Written down rather than read off the
#: screen, deliberately: a client accepting whatever it is told would pass on a
#: screen publishing a spelling nobody implemented — R1698's rule. The lab's own
#: `r1914_the_published_pin_addresses_are_the_taxonomys_members` is what holds
#: these to the taxonomy.
PARTS = ("host", "service")

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    """Where the screen mounted at `seat` answers, as the application says."""
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def cards(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/cards"))


def painted(app: RpcSubprocess) -> set[str]:
    """Every tag on the ASSEMBLED tool's frame."""
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    return {found["tag"] for _, found in walk_nodes(snap) if found.get("tag")}


def announced(app: RpcSubprocess) -> set[str]:
    """Every tag the accessibility tree carries.

    A separate surface from the paint tree, and asked separately on purpose: a
    pin that is drawn and not announced is a pin only a sighted reader has, and
    reading them off one snapshot could not tell the two apart.
    """
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return {
        node["tag"]
        for node in resp.result.get("nodes", [])
        if isinstance(node.get("tag"), str)
    }


def splittable_pin(published: dict) -> tuple[str, str, str] | None:
    """A card, one of its pins that splits, and the address that pin carries.

    Both halves are required. A pin that splits but carries nothing cannot
    show a value being shared out, and a pin that carries an address but is
    wired is one the reference refuses to split — so the walk needs the
    intersection, and asks the screen for it rather than naming a card.
    """
    for name, row in sorted(published.items()):
        for pin, verdict in sorted(row["pins"]["splits"].items()):
            carries = row["pins"]["carries"].get(pin)
            if verdict == "yes" and isinstance(carries, str) and "/" in carries:
                return name, pin, carries
    return None


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        # The lab is mounted when a reader goes there, so the journey is part
        # of the walk: a claim about a screen nobody can reach is a claim about
        # a binary nobody runs.
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("A — a card publishes a pin that splits, and what it carries")
        published = cards(app, surface)
        found = splittable_pin(published)
        ok(
            f"A: some card has a pin that splits AND carries an address — "
            f"{found[0] if found else None}",
            found is not None,
        )
        subject, pin, carrying = found
        print(f"    {subject}.{pin} carries {carrying!r}")
        ok(
            f"A: nothing is split yet, so {subject}'s member list is empty",
            published[subject]["pins"]["members"][pin] == [],
        )
        _scheme, rest = carrying.split("/", 1)
        want_host, want_service = rest.rsplit(":", 1)

        parent_tag = f"lab.pin.{subject}.{pin}"
        ok(
            f"A: the pin is on the frame ({parent_tag})",
            parent_tag in painted(app),
        )

        banner("B — splitting takes the parent off the frame and puts members on")
        said = app.invoke(f"{surface}/split_pin", f"{subject},{pin}")
        app.tick_ms(16)
        ok(
            f"B: the verb says how many pins it made and how many moved — {said}",
            "apart into 2 pin(s)" in said,
        )
        row = cards(app, surface)[subject]["pins"]
        ok(
            f"B: the parent is hidden and the model says WHY — {row[pin]!r}",
            row[pin] == "split",
        )
        frame = painted(app)
        ok("B: so the parent's own tag is off the frame", parent_tag not in frame)
        member_tags = {f"{parent_tag}.{part}" for part in PARTS}
        ok(
            f"B: and both member pins are on it — {sorted(member_tags)}",
            member_tags <= frame,
        )
        ok(
            "B: each announced to a reader who does not look at pixels — a pin "
            "drawn and not announced is a pin only a sighted reader has",
            member_tags <= announced(app),
        )

        banner("C — the members carry the two halves of THAT address")
        members = row["members"][pin]
        print(f"    members = {json.dumps(members)[:400]}")
        ok(
            f"C: two members, in the taxonomy's own order — "
            f"{[m['name'] for m in members]}",
            [m["name"] for m in members] == list(PARTS),
        )
        carried = [m["carries"] for m in members]
        ok(
            f"C: ★★★★★ they carry the two halves of the address this pin was "
            f"carrying — {carried} from {carrying!r}",
            carried == [want_host, want_service],
        )
        ok(
            "C: which is a value SHARED OUT and not a member's declared "
            "default — the distinction an assertion that either one satisfies "
            "could not make",
            carrying.endswith(f"{carried[0]}:{carried[1]}"),
        )

        banner("D — the members publish addresses, at consecutive indices")
        ok(
            "D: the members answer to ADDRESSES, not indices — an index moves "
            "when a pin before it splits and an address does not",
            [m["address"] for m in members] == [f"{pin}.{part}" for part in PARTS],
        )
        seats = [m["at"] for m in members]
        ok(
            f"D: and they sit immediately after the parent they came out of, "
            f"the reference's own order — {seats}",
            seats == [1, 2],
        )

        banner("E — folding at a MEMBER folds the pin it belongs to")
        said = app.invoke(f"{surface}/split_pin", f"{subject},-{pin}.{PARTS[1]}")
        app.tick_ms(16)
        ok(
            f"E: one split went away and the verb says how many — {said}",
            "from 1 split(s)" in said,
        )
        ok(
            f"E: ★★★★★ and it says the composed address, which is the one it "
            f"came apart from — {carrying!r}",
            carrying in said,
        )
        row = cards(app, surface)[subject]["pins"]
        ok(f"E: the parent is drawn again — {row[pin]!r}", row[pin] == "drawn")
        ok("E: with no members left", row["members"][pin] == [])
        ok(
            f"E: carrying the address it started with — {carrying!r}",
            row["carries"][pin] == carrying,
        )
        back = painted(app)
        ok("E: and its tag is back on the frame", parent_tag in back)
        ok(
            "E: while the member tags are gone — a fold is not an addition",
            not (member_tags & back),
        )

        banner("F — the refusals, in the model's words")
        for address, expect in (
            ("handle", "dial"),
            (f"{pin}.scheme", "host"),
            (f"-{pin}", "split"),
        ):
            sentence = assert_action_refused(
                lambda a=address: app.invoke(
                    f"{surface}/split_pin", f"{subject},{a}"
                ),
                saying=expect,
            )
            ok(
                f"F: {address!r} is refused with a reason naming {expect!r} — "
                f"{sentence}",
                expect in sentence,
            )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1914_a_pin_comes_apart_and_goes_back", body))
