#!/usr/bin/env python3
"""R1913 §5.32 §5.12 §2 #2 §2 #7 — **the assembled tool says of every pin
whether it splits, and when it does not, WHICH reason.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives `debt-node-system-coverage-campaign`, whose
census names five split/recombine rows; R1912 closed the QUESTION and this
round built the value half of the act.

# ★★★★★ Why a boolean would have passed here and a reason does not

The reference answers *can this pin be split* as one boolean over five
conditions:

    the pin is this node's, it is connectable, NOTHING IS LINKED TO IT,
    its type is a struct — and its base class says no, so a kind opts in

with a sixth in its schema at the moment of splitting: not a container. A
caller told `false` learns nothing about which failed, and the repairs are
entirely different — unplug the wire, pick another port, or accept that this
type has no members.

**Two of those reasons are reachable on this screen**, which is what makes this
walk about the distinction rather than about a constant: a card's wired pin
answers `wired` and an unwired one answers `atom`. One boolean cannot tell
them apart; a client offering "split this" needs to.

# What the tool owes beyond this, measured and NOT faked here

The act itself — the member ports, the parent hiding, the value distributed
across them, and the TREE the reference's recombine walks — needs an ADDRESS
this crate does not have: a socket is a node and a port INDEX, so nothing can
name member 1 of port 2, and a member port is a place a wire lands. That is
recorded on the four census rows rather than half-built here.

# What this walk holds

  (A) the assembled tool mounts the lab, and every card publishes for each pin
      whether it splits — a WORD, from the model's own vocabulary.
  (B) the words are ones the model can produce: no screen-side spelling.
  (C) BOTH reachable reasons actually occur on this screen, so the walk is
      about the distinction rather than about a constant.
  (D) the reason tracks the WIRING: the pins that answer `wired` are exactly
      the pins the tool reports as wired, which is the reference's own
      condition and the one a reading of the split alone would miss.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1913_a_pin_says_whether_it_splits.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"

#: The reasons the model can answer with. Written down rather than derived from
#: what the screen says, deliberately: a client accepting any word would pass on
#: a screen publishing a spelling nobody implemented — R1698's rule.
WORDS = {
    "yes",
    "no_such_node",
    "no_such_port",
    "control",
    "wired",
    "atom",
    "container",
}

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

        banner("A/B — every pin publishes a word from the model's vocabulary")
        cards = js(app.query(f"{surface}/cards"))
        ok(f"A: the lab publishes {len(cards)} card(s)", len(cards) > 1)
        seen: dict[str, set[str]] = {}
        for name, row in cards.items():
            splits = row["pins"].get("splits")
            ok(
                f"A: {name} says whether each pin splits — {splits}",
                isinstance(splits, dict) and {"dial", "accept"} <= set(splits),
            )
            for pin, word in splits.items():
                ok(
                    f"B: {name}.{pin} answers {word!r}, a word the model can "
                    f"produce",
                    word in WORDS,
                )
                seen.setdefault(word, set()).add(f"{name}.{pin}")

        banner("C — both reachable reasons actually occur")
        print(f"    {json.dumps({k: sorted(v) for k, v in seen.items()})[:400]}")
        ok(
            f"C: more than one reason occurs on this screen — {sorted(seen)}",
            len(seen) > 1,
        )
        ok(
            "C: and `wired` is one of them, which one boolean could not have "
            "told from the other",
            "wired" in seen,
        )
        ok(
            "C: as is `atom` — this screen's types have no members, and that "
            "is a DIFFERENT answer from a wire being in the way",
            "atom" in seen,
        )

        banner("D — the reason tracks the wiring the tool reports")
        for name, row in cards.items():
            wired = set(row["pins"]["wired"])
            said = {
                pin
                for pin, word in row["pins"]["splits"].items()
                if word == "wired"
            }
            ok(
                f"D: {name}: pins answering `wired` are exactly the wired pins "
                f"— {sorted(said)} vs {sorted(wired)}",
                said == wired,
            )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1913_a_pin_says_whether_it_splits", body))
