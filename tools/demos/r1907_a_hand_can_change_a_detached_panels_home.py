#!/usr/bin/env python3
"""R1907 §5.16 §5.21 §2 #7 — **a detached panel's home is a thing a HAND can
change, and the policy is what says where "somewhere else" is.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This one closes the last item the arrangeable-unit
campaign named: R1891 gave a torn-off card a home, R1905 made the geometry
follow it, and after both the home was still reachable only over the wire.

    the only non-test caller of the screen's verb was the wire dispatch

⇒ *a value a person cannot change is a value the person does not have.* That is
the same defect this axis recorded at R1802, one gesture over, and the same one
R1902 met when a pane's opening placement was two constants nobody could reach.

# ★★★★★ What the behaviour canon says, measured before any of this was built

The canon's board card wires settings, detach and remove; its detached panel
wires re-dock and close. It has **no** home to choose: a detached panel there is
one absolutely-positioned frame and there is nowhere else for it to be. So this
walk is NOT a first-pass reproduction — the standing order rule says the canon
is a floor and not a ceiling, and what this tree has that the canon does not is
a second home (the terminal backends have no window server, §2 #6) and therefore
a real choice to offer.

The floor toolkit at 6.11 is in the same position for the same reason: a
detached panel there is always a top-level window, so nothing on that class
names a choice about where one lives.

# What this walk holds

  (A) the assembled tool publishes what a detached panel's HEADER offers, and
      where "the next home" leads FROM EACH home — so a client knows what the
      control will do before pressing it, rather than after.
  (B) a press on that control moves the panel to exactly the home the published
      map named. Not a home this walk chose: the assertion reads the map.
  (C) the geometry followed, and the crossing says how it arrived — R1905's
      seam, consulted by this new channel and not only by the wire.
  (D) pressing it again returns the panel, so an unfamiliar mark is safe to try.
  (E) the wire can say `next` too, so the two channels are one verb rather than
      a hand-path and an agent-path that can drift.
  (F) a home this host does not have is still refused BY NAME, so adding the
      new request word did not turn the parser permissive.

# What this walk deliberately does NOT hold

That a host with ONE home draws no control and refuses `next` with its own
sentence. This host has two by construction, and building a one-home host here
would be a fixture rather than the assembled tool. That pair lives in the
in-process gates, where the policy can be constructed directly:
`r1907_a_host_with_one_home_has_no_next_one_and_says_so_in_its_own_words` and
`r1907_the_send_home_control_exists_exactly_where_there_is_a_second_home`.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1907_a_hand_can_change_a_detached_panels_home.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import abs_rects_of, run_demo, RpcSubprocess  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
CARD = "packet#0"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, name: str):
    """One published slot, decoded."""
    value = app.query(f"{EXT}/{name}")
    return json.loads(value) if isinstance(value, str) else value


def settle(app: RpcSubprocess) -> None:
    for _ in range(6):
        app.tick_ms(16)


def float_of(app: RpcSubprocess, card: str) -> dict:
    """The one detached card this walk is about."""
    floats = q(app, "floats")
    found = [f for f in floats if f["id"] == card]
    assert found, f"{card!r} must be among the floats: {floats}"
    return found[0]


def on_the_canvas(app: RpcSubprocess) -> dict:
    """Tear the card off and put it where this screen paints it."""
    app.invoke(f"{EXT}/act", f"{CARD},tear_off")
    settle(app)
    app.invoke(f"{EXT}/detach_home", f"{CARD},canvas")
    settle(app)
    return float_of(app, CARD)


def section_a(app: RpcSubprocess) -> dict:
    banner("A — the tool publishes what the header offers, and where next leads")
    policy = q(app, "detach_policy")
    ok(
        f"A: the host declares more than one home, so a choice exists — "
        f"{policy['homes']}",
        len(policy["homes"]) > 1,
    )
    ok(
        f"A: ** and it publishes what a detached panel's HEADER offers — "
        f"{policy.get('affordances')}",
        "send_home" in (policy.get("affordances") or []),
    )
    nxt = policy.get("next_from") or {}
    ok(
        f"A: ***** and where 'the next home' leads FROM EACH home — {nxt}. A "
        "client that had to derive this would be holding a second copy of the "
        "host's policy, free to disagree the day a third home appears",
        set(nxt) == set(policy["homes"]),
    )
    ok(
        "A: * every named destination is a home this host admits, so the "
        f"control cannot offer a place the policy would refuse — {nxt}",
        all(to in policy["homes"] for to in nxt.values()),
    )
    ok(
        f"A: * and no home leads to itself, or 'somewhere else' is a lie — {nxt}",
        all(frm != to for frm, to in nxt.items()),
    )
    return policy


def section_b(app: RpcSubprocess, policy: dict) -> dict:
    banner("B — a PRESS on the header control moves the panel")
    before = on_the_canvas(app)
    ok(
        f"B: the panel is on the canvas, which is where this screen paints it — "
        f"{before['home']!r} / {before['space']!r}",
        before["home"] == "canvas" and before["space"] == "host",
    )
    drawn = abs_rects_of(app.snapshot(source="paint"))
    tag = f"float.{CARD}.send_home"
    ok(
        f"B: ** the control is DRAWN, so a person can see the capability — "
        f"{tag} at {drawn.get(tag)}",
        tag in drawn and drawn[tag][2] > 0 and drawn[tag][3] > 0,
    )
    expected = (policy.get("next_from") or {})[before["home"]]
    app.click(path=tag)
    settle(app)
    after = float_of(app, CARD)
    ok(
        f"B: ***** the press sent it to the home the PUBLISHED MAP named — "
        f"{before['home']!r} -> {after['home']!r}, expected {expected!r}. "
        "Unchanged here is the state R1891 left, not a pass",
        after["home"] == expected,
    )
    ok(
        f"B: ** and the space followed the home, so the rectangle is read "
        f"against the right origin — {after['space']!r}",
        after["space"] == "display",
    )
    return before


def section_c(app: RpcSubprocess, before: dict) -> None:
    banner("C — the crossing went through R1905's seam, not around it")
    arrival = q(app, "arrival")
    ok(
        f"C: ** the crossing published how it arrived — {arrival}. A hand-path "
        "that skipped the transfer would leave this null while the home moved",
        arrival is not None,
    )
    ok(
        f"C: ***** and it CONVERTED rather than crossing unconverted — "
        f"knows_offset={arrival['knows_offset']}, how={arrival['how']!r}. "
        "`adrift` here would mean this channel reaches the home without "
        "reaching the geometry",
        arrival["knows_offset"] is True and arrival["how"] != "adrift",
    )
    after = float_of(app, CARD)
    ok(
        f"C: * and the numbers moved, which is what 'converted' means — "
        f"({before['x']}, {before['y']}) -> ({after['x']}, {after['y']})",
        (after["x"], after["y"]) != (before["x"], before["y"])
        or arrival["how"] == "kept",
    )


def section_d(app: RpcSubprocess, before: dict) -> None:
    banner("D — pressing it again brings the panel back")
    # ⚠ The panel is window-homed now, so this screen does not paint its header:
    # a real window carries it. The control is pressed where it IS — which is
    # why this leg drives the verb through the same request the control makes
    # rather than through a click on a rectangle that is in another window.
    said = app.invoke(f"{EXT}/detach_home", f"{CARD},next")
    ok(f"D: the host admits the request — {said!r}", said is not None)
    settle(app)
    back = float_of(app, CARD)
    ok(
        f"D: ***** two turns of one control are the identity, so an unfamiliar "
        f"mark is safe to try — {before['home']!r} -> {back['home']!r}",
        back["home"] == before["home"],
    )
    ok(
        f"D: * and the panel is painted again where a hand can reach it — "
        f"{back['space']!r}",
        back["space"] == "host",
    )


def section_e(app: RpcSubprocess) -> None:
    banner("E — the wire and the hand are ONE verb")
    drawn = abs_rects_of(app.snapshot(source="paint"))
    tag = f"float.{CARD}.send_home"
    ok(f"E: the control is painted again — {tag}", tag in drawn)
    by_wire = app.invoke(f"{EXT}/detach_home", f"{CARD},next")
    settle(app)
    wire_home = float_of(app, CARD)["home"]
    app.invoke(f"{EXT}/detach_home", f"{CARD},canvas")
    settle(app)
    app.click(path=tag)
    settle(app)
    hand_home = float_of(app, CARD)["home"]
    ok(
        f"E: ***** the wire's `next` and the hand's press land in the SAME "
        f"home — {wire_home!r} / {hand_home!r} (wire said {by_wire!r}). Two "
        "paths that did this differently would be two behaviours, and only one "
        "of them gets tested",
        wire_home == hand_home,
    )


def section_f(app: RpcSubprocess) -> None:
    banner("F — a word the host does not know is still refused by name")
    app.invoke(f"{EXT}/detach_home", f"{CARD},canvas")
    settle(app)
    refused = None
    try:
        app.invoke(f"{EXT}/detach_home", f"{CARD},elsewhere")
    except Exception as why:  # noqa: BLE001 — the refusal is the subject
        refused = str(why)
    ok(
        f"F: ***** adding `next` did not make the parser permissive — "
        f"{refused!r}",
        refused is not None and "elsewhere" in refused,
    )
    ok(
        f"F: ** and the sentence names what WOULD have worked, so a reader is "
        f"not left guessing — {refused!r}",
        refused is not None and "next" in refused,
    )
    still = float_of(app, CARD)
    ok(
        f"F: * a refused request moved nothing — {still['home']!r}",
        still["home"] == "canvas",
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        policy = section_a(app)
        before = section_b(app, policy)
        section_c(app, before)
        section_d(app, before)
        section_e(app)
        section_f(app)
        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1907 a hand can change a detached panel's home", body)
